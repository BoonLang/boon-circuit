use boon_document::{
    DocumentFrame, DocumentNodeId, DocumentNodeKind, DocumentState, LayoutDemand, StylePatch,
    StyleValue, TextValue,
};
use boon_editor::{Buffer, Command, Position};
use boon_host::{
    DocumentNodeId as HostDocumentNodeId, HostEvent, PointerButton, PointerPhase, SourceBindingId,
};
use boon_persistence::{
    InMemoryDriver, MigrationPreview, OutboxInspectorState, PersistenceInspectorSnapshot,
    PersistenceWorkerConfig, PersistenceWorkerStatus, RedbDriver,
};
use boon_plan::{
    ApplicationIdentity, ApplicationPlan, EffectReplay, FiniteReal, MachinePlan, MemoryKind,
    ProgramRole,
};
pub(crate) use boon_runtime::ProgramCompletionObservation;
use boon_runtime::{
    DistributedProgramBundle, DocumentPatch, DocumentPatchStatus, HostEffectRouter,
    HostEffectWorker, LiveRuntime, ObservedProgramCompletion, PersistentRuntime,
    PersistentRuntimeStartup, PersistentRuntimeStartupDisposition, ProgramArtifact,
    ProgramArtifactDrive, ProgramArtifactLaneKind, ProgramArtifactLaneOutcome,
    ProgramArtifactTurnKind, ProgramDiagnostic, ProgramDocumentHost, ProgramHostDiagnostic,
    ProgramHostRequest, ProgramRejection, ProgramRequestId, ProgramSessionId, RowId,
    RuntimePhaseTimings, RuntimeTurn, SessionOptions, SourcePayload, Value,
};
use boon_server_runtime::{
    InProcessDistributedRuntime, InProcessPoll, InProcessTransientEffectOwner,
    PersistentServerConfig, PersistentServerStartup,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::protocol::{
    AssetBlob, AuthoritySelection, AuthoritySelectionKind, AuthoritySummary, DurableSummary,
    MAX_PERSISTENCE_OUTBOX_SAMPLES, MAX_PERSISTENCE_STATUS_BYTES, OutboxSample, OutboxSampleState,
    OutboxSummary, PendingSummary, PersistenceCapabilities, PersistenceCapability,
    PersistenceOperationStatus, PersistenceSnapshot, PersistenceTimingSummary,
    StateArtifactPreviewSummary, StoredSummary,
};
use crate::transient_host::{NativeTransientHost, PackageAsset, TransientHostCompletion};
use crate::view::HitTarget;
type ViewResult<T> = Result<T, String>;

const DOUBLE_CLICK_INTERVAL: Duration = Duration::from_millis(500);
const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);
const PERSISTENCE_ACK_POLL_INTERVAL: Duration = Duration::from_millis(25);
const STATE_DIRECTORY: &str = "playground/state";
pub(crate) const STATE_ROOT_ENV: &str = "BOON_PLAYGROUND_STATE_ROOT";
const EFFECT_POLL_INTERVAL: Duration = Duration::from_millis(1);
const MAX_TRANSIENT_COMPLETIONS_PER_POLL: usize = 8;
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
    runtime: RuntimeBackend,
    machine_plan: Arc<MachinePlan>,
    program_host: ProgramDocumentHost,
    application: ApplicationIdentity,
    persistence_schema_version: u64,
    persistence_schema_hash: [u8; 32],
    startup: RuntimeStartup,
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
    transient_host: NativeTransientHost,
    next_effect_poll: Option<Instant>,
    distributed_started: Option<Instant>,
    distributed_effect_owners:
        BTreeMap<boon_runtime::TransientEffectCallId, InProcessTransientEffectOwner>,
    host_identity_mode: HostIdentityMode,
    host_identity_generation: u64,
    scenario_trigger_source: Option<String>,
    scenario_trigger_turn: Option<RuntimeTurn>,
    pending_durable_lanes: BTreeMap<u64, PendingDurableLane>,
    async_lane_observations: Vec<RuntimeAsyncLaneObservation>,
}

enum RuntimeBackend {
    Single(PersistentRuntime),
    Distributed(InProcessDistributedRuntime),
}

enum RuntimeStartup {
    Single(PersistentRuntimeStartup),
    Distributed(PersistentServerStartup),
}

pub(crate) struct RuntimeStartupEvidence {
    pub disposition: PersistentRuntimeStartupDisposition,
    pub schema_version: u64,
    pub schema_hash: [u8; 32],
}

impl RuntimeBackend {
    fn single(&self) -> ViewResult<&PersistentRuntime> {
        match self {
            Self::Single(runtime) => Ok(runtime),
            Self::Distributed(_) => Err(
                "single-role persistence operation is unavailable for a distributed package"
                    .to_owned(),
            ),
        }
    }

    fn single_mut(&mut self) -> ViewResult<&mut PersistentRuntime> {
        match self {
            Self::Single(runtime) => Ok(runtime),
            Self::Distributed(_) => Err(
                "single-role persistence operation is unavailable for a distributed package"
                    .to_owned(),
            ),
        }
    }

    fn distributed_mut(&mut self) -> Option<&mut InProcessDistributedRuntime> {
        match self {
            Self::Single(_) => None,
            Self::Distributed(runtime) => Some(runtime),
        }
    }

    fn is_distributed(&self) -> bool {
        matches!(self, Self::Distributed(_))
    }
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

pub struct RuntimePlanChange {
    pub target_schema_version: u64,
    pub durable_epoch: u64,
    pub through_turn_sequence: u64,
    pub migration: Option<MigrationPreview>,
}

impl RuntimeView {
    pub fn open_with_assets(
        plan: Arc<MachinePlan>,
        deterministic: bool,
        assets: &[AssetBlob],
    ) -> ViewResult<Self> {
        let identity_mode = match deterministic {
            true => HostIdentityMode::Deterministic,
            false => HostIdentityMode::Interactive,
        };
        Self::open_with_state_root_and_identity_mode(
            plan,
            configured_state_root(),
            identity_mode,
            assets,
        )
    }

    pub fn open_for_scenario_with_assets(
        plan: Arc<MachinePlan>,
        assets: &[AssetBlob],
    ) -> ViewResult<Self> {
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
        Self::mount_persistent(
            runtime,
            mount,
            host_started,
            startup,
            HostIdentityMode::Deterministic,
            host_identity_generation,
            transient_content_root(&repository_root().join("target/boon-transient")),
            assets,
        )
    }

    pub fn open_distributed_with_assets(
        bundle: DistributedProgramBundle,
        deterministic: bool,
        assets: &[AssetBlob],
    ) -> ViewResult<Self> {
        let client_artifact = bundle
            .artifact(ProgramRole::Client)
            .ok_or_else(|| "distributed package has no Client artifact".to_owned())?;
        let server_artifact = bundle
            .artifact(ProgramRole::Server)
            .ok_or_else(|| "distributed package has no Server artifact".to_owned())?;
        let machine_plan = client_artifact.plan().clone();
        validate_preview_plan(&machine_plan)?;
        let application = client_artifact.application().clone();
        let authority_plan = server_artifact.plan().clone();
        let required_effects = bundle
            .artifacts()
            .iter()
            .flat_map(|artifact| transient_effect_ids(artifact.plan()))
            .collect::<BTreeSet<_>>();
        let state_root = if deterministic {
            transient_content_root(&repository_root().join("target/boon-distributed-state"))
        } else {
            configured_state_root()
        };
        let database_path = state_database_path_in(&state_root, server_artifact.application())?;
        let database_parent = database_path
            .parent()
            .ok_or_else(|| "distributed Server database has no parent directory".to_owned())?;
        fs::create_dir_all(database_parent).map_err(|error| {
            format!(
                "create distributed Server state directory `{}`: {error}",
                database_parent.display()
            )
        })?;
        let driver = RedbDriver::open(&database_path).map_err(|error| {
            format!(
                "open distributed Server state database `{}`: {error}",
                database_path.display()
            )
        })?;
        let (mut runtime, startup) = InProcessDistributedRuntime::start_persistent(
            &bundle,
            driver,
            PersistentServerConfig::authoritative(PersistenceWorkerConfig::default()),
        )
        .map_err(|error| error.to_string())?;
        let persistence_status = startup.lifecycle.status().persistence;
        let content_root = transient_content_root(&state_root.join("transient"));
        let mut transient_host = NativeTransientHost::new(
            content_root,
            assets.iter().map(|asset| PackageAsset {
                url: &asset.url,
                media: &asset.media_type,
                bytes: &asset.bytes,
            }),
            required_effects,
        )?;
        let distributed_started = Instant::now();
        let mut distributed_effect_owners = BTreeMap::new();
        let mut initial_client_turns = Vec::new();
        for _ in 0..1_024 {
            let poll = runtime
                .poll(Duration::ZERO)
                .map_err(|error| error.to_string())?;
            route_distributed_transient_effects(
                &mut transient_host,
                &mut distributed_effect_owners,
                &poll,
            )?;
            initial_client_turns.extend(poll.client_turns);
            if !poll.has_more_work {
                break;
            }
        }
        if runtime.next_deadline() == Some(Duration::ZERO) {
            return Err("distributed package mount exceeded the bounded startup pump".to_owned());
        }
        let frame = runtime
            .document_frame()
            .cloned()
            .ok_or_else(|| "distributed Client produced no retained document".to_owned())?;
        let (program_host, program_requests) =
            ProgramDocumentHost::mount(application.clone(), &frame);
        if !program_requests.is_empty() {
            return Err(
                "distributed Client document requested nested Program artifacts before that owner is installed"
                    .to_owned(),
            );
        }
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
        let runtime_turn_sequence = initial_client_turns
            .iter()
            .map(|turn| turn.sequence)
            .max()
            .unwrap_or(0);
        let sequence = initial_client_turns
            .iter()
            .filter_map(|turn| turn.source_sequence)
            .max()
            .unwrap_or(0);
        let last_runtime_phase = initial_client_turns
            .last()
            .map_or_else(RuntimePhaseTimings::default, |turn| turn.phase_timings);
        let host_identity_mode = if deterministic {
            HostIdentityMode::Deterministic
        } else {
            HostIdentityMode::Interactive
        };
        let mut view = Self {
            runtime: RuntimeBackend::Distributed(runtime),
            machine_plan: machine_plan.clone(),
            program_host,
            application,
            persistence_schema_version: authority_plan.persistence.schema_version,
            persistence_schema_hash: authority_plan.persistence.schema_hash,
            startup: RuntimeStartup::Distributed(startup),
            authority_plan_counts: authority_plan_counts(&authority_plan),
            authority_selections: authority_selections(&authority_plan),
            persistence_status,
            persistence_inspector: None,
            persistence_inspector_error: None,
            next_persistence_poll: None,
            runtime_turn_sequence,
            hovered: None,
            pressed: None,
            focused: None,
            text_inputs,
            text_drag: None,
            modifiers: InputModifiers::default(),
            clipboard_fallback: None,
            clipboard_system_synchronized: false,
            scroll_offsets: BTreeMap::new(),
            materialization_overscan: BTreeMap::new(),
            pending_patches: Vec::new(),
            sequence,
            event_dispatches: None,
            pending_external_url: None,
            last_primary_click: None,
            last_runtime_phase,
            scheduled_sources: scheduled_sources_from_plan(&machine_plan)?,
            effect_worker: native_effect_worker()?,
            transient_host,
            next_effect_poll: None,
            distributed_started: Some(distributed_started),
            distributed_effect_owners,
            host_identity_mode,
            host_identity_generation: 1,
            scenario_trigger_source: None,
            scenario_trigger_turn: None,
            pending_durable_lanes: BTreeMap::new(),
            async_lane_observations: Vec::new(),
        };
        view.dispatch_host_lifecycle_started()?;
        view.schedule_effect_poll()?;
        view.pending_patches.clear();
        Ok(view)
    }

    pub(crate) fn open_with_state_root_deterministic(
        plan: Arc<MachinePlan>,
        state_root: impl AsRef<Path>,
    ) -> ViewResult<Self> {
        Self::open_with_state_root_and_identity_mode(
            plan,
            state_root,
            HostIdentityMode::Deterministic,
            &[],
        )
    }

    fn open_with_state_root_and_identity_mode(
        plan: Arc<MachinePlan>,
        state_root: impl AsRef<Path>,
        host_identity_mode: HostIdentityMode,
        assets: &[AssetBlob],
    ) -> ViewResult<Self> {
        validate_preview_plan(&plan)?;
        let content_root = transient_content_root(&state_root.as_ref().join("transient"));
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
        Self::mount_persistent(
            runtime,
            mount,
            host_started,
            startup,
            host_identity_mode,
            host_identity_generation,
            content_root,
            assets,
        )
    }

    fn mount_persistent(
        mut runtime: PersistentRuntime,
        turn: RuntimeTurn,
        host_started: Option<RuntimeTurn>,
        startup: PersistentRuntimeStartup,
        host_identity_mode: HostIdentityMode,
        host_identity_generation: u64,
        content_root: PathBuf,
        assets: &[AssetBlob],
    ) -> ViewResult<Self> {
        let source_sequence = source_sequence_after_turn(
            source_sequence_after_turn(0, turn.source_sequence),
            host_started.as_ref().and_then(|turn| turn.source_sequence),
        );
        let runtime_turn_sequence = host_started
            .as_ref()
            .map_or(turn.sequence, |turn| turn.sequence);
        let mut transient_host = NativeTransientHost::new(
            content_root,
            assets.iter().map(|asset| PackageAsset {
                url: &asset.url,
                media: &asset.media_type,
                bytes: &asset.bytes,
            }),
            transient_effect_ids(runtime.runtime().machine_plan()),
        )?;
        if let Some(host_started) = &host_started {
            transient_host.route_turn(host_started)?;
        }
        transient_host.route_turn(&turn)?;
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
        runtime.queue_program_requests(pending_program_requests);
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
        let machine_plan = runtime.runtime().shared_machine_plan();
        let plan = machine_plan.as_ref();
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
        let next_effect_poll =
            (runtime.has_effect_work() || transient_host.has_work()).then_some(Instant::now());
        let mut view = Self {
            runtime: RuntimeBackend::Single(runtime),
            machine_plan,
            program_host,
            application,
            persistence_schema_version,
            persistence_schema_hash,
            startup: RuntimeStartup::Single(startup),
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
            transient_host,
            next_effect_poll,
            distributed_started: None,
            distributed_effect_owners: BTreeMap::new(),
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
        self.machine_plan.clone()
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
            .single()?
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
            .single_mut()?
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
        let plan = self.machine_plan.clone();
        self.activate_machine_plan(plan)
    }

    pub fn start_over(&mut self) -> ViewResult<RuntimePlanChange> {
        let plan = self.machine_plan.clone();
        let target_schema_version = plan.persistence.schema_version;
        let reset = self
            .runtime
            .single_mut()?
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
        self.transient_host.route_turn(&mount)?;
        self.runtime.single_mut()?.reset_program_artifacts();
        let runtime_turn_sequence = mount.sequence;
        if mount.document_patch_status != DocumentPatchStatus::Complete {
            return Err("MachinePlan did not produce complete typed document bindings".to_owned());
        }
        let mounted = state_from_mount(mount.document_patches)?;
        let frame = self
            .runtime
            .single()?
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?
            .clone();
        debug_assert_eq!(mounted.frame(), &frame);

        let (program_host, pending_program_requests) =
            ProgramDocumentHost::mount(self.application.clone(), &frame);
        self.program_host = program_host;
        self.runtime
            .single_mut()?
            .queue_program_requests(pending_program_requests);
        self.resolve_program_artifact_requests_blocking()?;
        self.pending_patches.clear();
        let frame = self.program_host.frame().clone();
        self.retain_view_state(&frame, true);
        self.materialization_overscan.clear();
        self.pending_patches.clear();
        self.last_runtime_phase = RuntimePhaseTimings::default();
        self.refresh_plan_metadata();
        self.scheduled_sources = scheduled_sources_from_plan(&self.machine_plan)?;
        self.runtime_turn_sequence = runtime_turn_sequence;
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

    pub(crate) fn startup_evidence(&self) -> RuntimeStartupEvidence {
        match &self.startup {
            RuntimeStartup::Single(startup) => RuntimeStartupEvidence {
                disposition: startup.disposition.clone(),
                schema_version: startup.restore_image.schema_version,
                schema_hash: startup.restore_image.schema_hash,
            },
            RuntimeStartup::Distributed(startup) => RuntimeStartupEvidence {
                disposition: startup.disposition.clone(),
                schema_version: self.persistence_schema_version,
                schema_hash: self.persistence_schema_hash,
            },
        }
    }

    pub(crate) fn parent_runtime_generation(&self) -> u64 {
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime.generation(),
            RuntimeBackend::Distributed(_) => 1,
        }
    }

    pub fn authority_selection_for_path(&self, path: &str) -> Option<AuthoritySelection> {
        self.authority_selections.get(path).cloned()
    }

    pub fn runtime_turn_sequence(&self) -> u64 {
        self.runtime_turn_sequence
    }

    pub fn semantic_value_image(&self) -> ViewResult<boon_persistence::RestoreImage> {
        self.runtime.single()?.semantic_value_image()
    }

    pub fn assert_scenario_step(&mut self, step: &boon_runtime::ScenarioStep) -> ViewResult<()> {
        self.scenario_trigger_source = None;
        let turn = self.scenario_trigger_turn.take();
        self.runtime
            .single_mut()?
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
        let mut changed = if self.runtime.is_distributed() {
            self.poll_distributed_runtime(now)?
        } else {
            let turn = self
                .runtime
                .single_mut()?
                .poll_effect_worker(&mut self.effect_worker)
                .map_err(|error| error.to_string())?;
            turn.map_or(Ok(false), |turn| self.finish_parent_runtime_turn(turn))?
        };
        for _ in 0..MAX_TRANSIENT_COMPLETIONS_PER_POLL {
            let Some(completion) = self.transient_host.try_completion()? else {
                break;
            };
            if self.runtime.is_distributed() {
                self.complete_distributed_transient_effect(completion)?;
                changed |= self.poll_distributed_runtime(now)?;
            } else {
                let turn = match completion {
                    TransientHostCompletion::Single { call_id, outcome } => self
                        .runtime
                        .single_mut()?
                        .complete_transient_effect(call_id, outcome)
                        .map_err(|error| error.to_string())?,
                    TransientHostCompletion::File(event) => {
                        if event.is_stream() {
                            self.runtime
                                .single_mut()?
                                .deliver_transient_effect_result(
                                    event.call_id,
                                    event.result_sequence,
                                    event.outcome,
                                )
                                .map_err(|error| error.to_string())?
                        } else {
                            self.runtime
                                .single_mut()?
                                .complete_transient_effect(event.call_id, event.outcome)
                                .map_err(|error| error.to_string())?
                        }
                    }
                };
                changed |= self.finish_parent_runtime_turn(turn)?;
            }
        }
        self.schedule_effect_poll()?;
        Ok(changed)
    }

    fn complete_distributed_transient_effect(
        &mut self,
        completion: TransientHostCompletion,
    ) -> ViewResult<()> {
        let (call_id, terminal) = match &completion {
            TransientHostCompletion::Single { call_id, .. } => (*call_id, true),
            TransientHostCompletion::File(event) => (event.call_id, event.is_terminal()),
        };
        let owner = self
            .distributed_effect_owners
            .get(&call_id)
            .copied()
            .ok_or_else(|| format!("native host completed unowned distributed call {call_id}"))?;
        let runtime = self.runtime.distributed_mut().ok_or_else(|| {
            "distributed effect completion reached a single-role runtime".to_owned()
        })?;
        match completion {
            TransientHostCompletion::Single { outcome, .. } => runtime
                .complete_transient_effect(owner, call_id, outcome)
                .map_err(|error| error.to_string())?,
            TransientHostCompletion::File(event) if event.is_stream() => runtime
                .deliver_transient_effect_result(
                    owner,
                    call_id,
                    event.result_sequence,
                    event.outcome,
                )
                .map_err(|error| error.to_string())?,
            TransientHostCompletion::File(event) => runtime
                .complete_transient_effect(owner, call_id, event.outcome)
                .map_err(|error| error.to_string())?,
        }
        if terminal {
            self.distributed_effect_owners.remove(&call_id);
        }
        Ok(())
    }

    fn poll_distributed_runtime(&mut self, now: Instant) -> ViewResult<bool> {
        let started = self
            .distributed_started
            .ok_or_else(|| "distributed runtime has no logical clock origin".to_owned())?;
        let logical_now = now.saturating_duration_since(started);
        let poll = self
            .runtime
            .distributed_mut()
            .ok_or_else(|| "distributed poll reached a single-role runtime".to_owned())?
            .poll(logical_now)
            .map_err(|error| error.to_string())?;
        route_distributed_transient_effects(
            &mut self.transient_host,
            &mut self.distributed_effect_owners,
            &poll,
        )?;
        let mut parent_patches = Vec::new();
        for turn in poll.client_turns {
            self.runtime_turn_sequence = self.runtime_turn_sequence.max(turn.sequence);
            self.sequence = source_sequence_after_turn(self.sequence, turn.source_sequence);
            self.last_runtime_phase = turn.phase_timings;
            parent_patches.extend(turn.document_patches);
        }
        let parent = self
            .runtime
            .distributed_mut()
            .and_then(|runtime| runtime.document_frame().cloned())
            .ok_or_else(|| "distributed Client lost its retained document".to_owned())?;
        let update = self
            .program_host
            .reconcile_with_parent_patches(&parent, parent_patches);
        if !update.requests.is_empty() {
            return Err(
                "distributed Client requested nested Program artifacts without a distributed owner"
                    .to_owned(),
            );
        }
        let mut changed = self.queue_program_update(update.patches, Vec::new());
        changed |= self.dispatch_rejections(update.rejections)?;
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
            && match &self.runtime {
                RuntimeBackend::Single(runtime) => !runtime.program_artifacts_pending(),
                RuntimeBackend::Distributed(_) => true,
            };
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
                rebuild_derived_us: match &self.runtime {
                    RuntimeBackend::Single(runtime) => runtime.last_rebuild_derived_us(),
                    RuntimeBackend::Distributed(_) => 0,
                },
            },
            outbox,
            worker_alive: self.persistence_status.worker_alive,
            capabilities: PersistenceCapabilities {
                clear_selected: self.persistence_capability(),
                export_state: self.persistence_capability(),
                import_preview: self.persistence_capability(),
                activate_import: self.persistence_capability(),
            },
            import_preview,
            last_actionable_error,
            last_operation: last_operation.map(|mut operation| {
                operation.message = bounded_persistence_text(&operation.message);
                operation
            }),
        }
    }

    fn persistence_capability(&self) -> PersistenceCapability {
        if self.runtime.is_distributed() {
            PersistenceCapability {
                available: false,
                reason: "distributed Server persistence controls are not mounted in this preview"
                    .to_owned(),
            }
        } else {
            available_capability()
        }
    }

    pub fn flush_persistence(&mut self) -> ViewResult<(u64, u64)> {
        let acknowledgement = self
            .runtime
            .single()?
            .flush()
            .map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok((
            acknowledgement.epoch,
            self.persistence_status.durable_through_turn_sequence,
        ))
    }

    pub fn compact_persistence(&mut self) -> ViewResult<u64> {
        let acknowledgement = self
            .runtime
            .single()?
            .compact()
            .map_err(|error| error.to_string())?;
        self.refresh_persistence_after_control();
        Ok(acknowledgement.epoch)
    }

    pub fn export_state_artifact(&mut self) -> ViewResult<Vec<u8>> {
        let artifact = self
            .runtime
            .single()?
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
            .single()?
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
            .single_mut()?
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
            .single_mut()?
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
        let Ok(runtime) = self.runtime.single() else {
            return;
        };
        self.machine_plan = runtime.runtime().shared_machine_plan();
        let plan = self.machine_plan.as_ref();
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
        let inspection = match self.runtime.single() {
            Ok(runtime) => runtime.inspect(),
            Err(_) => return false,
        };
        match inspection {
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
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime.status(),
            RuntimeBackend::Distributed(runtime) => runtime
                .persistent_server_status()
                .map(|status| status.persistence)
                .unwrap_or_else(unavailable_persistence_status),
        }
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

    fn apply_program_artifact_drive(
        &mut self,
        drive: ProgramArtifactDrive,
    ) -> ViewResult<(bool, Option<ProgramCompletionObservation>)> {
        let mut changed = drive.changed;
        for artifact_turn in drive.turns {
            let source_sequence = artifact_turn.turn.source_sequence.ok_or_else(|| {
                format!(
                    "program artifact source dispatch `{}` produced no source sequence",
                    artifact_turn.source_path
                )
            })?;
            self.capture_scenario_turn(&artifact_turn.source_path, &artifact_turn.turn);
            self.sequence = source_sequence_after_turn(self.sequence, Some(source_sequence));
            self.last_runtime_phase = artifact_turn.turn.phase_timings;
            if artifact_turn.kind == ProgramArtifactTurnKind::Parent {
                self.transient_host.route_turn(&artifact_turn.turn)?;
                let durable_sequence = artifact_turn.turn.sequence;
                let queued_at = Instant::now();
                self.runtime_turn_sequence = durable_sequence;
                self.persistence_status = self.query_persistence_status();
                self.pending_durable_lanes.insert(
                    durable_sequence,
                    PendingDurableLane {
                        queued_at,
                        enqueue_us: artifact_turn.turn.phase_timings.persistence_enqueue_us,
                        queue_depth: self
                            .persistence_status
                            .queue_depth
                            .max(1)
                            .try_into()
                            .unwrap_or(u32::MAX),
                    },
                );
                self.record_durable_lane_completions(queued_at, 0);
                self.next_persistence_poll = Some(queued_at + PERSISTENCE_ACK_POLL_INTERVAL);
                self.schedule_effect_poll()?;
            }
            self.record_event_dispatch(&artifact_turn.source_path, source_sequence);
        }
        changed |= self.queue_program_update(drive.patches, Vec::new());
        self.async_lane_observations
            .extend(drive.observations.into_iter().map(|observation| {
                RuntimeAsyncLaneObservation {
                    lane: match observation.lane {
                        ProgramArtifactLaneKind::Store => {
                            RuntimeAsyncLaneKind::ProgramArtifactStore
                        }
                        ProgramArtifactLaneKind::Load => RuntimeAsyncLaneKind::ProgramArtifactLoad,
                    },
                    request_id: observation.request_id,
                    revision: observation.revision,
                    queue_depth: observation.queue_depth,
                    queue_wait_us: observation.queue_wait_us,
                    worker_us: observation.worker_us,
                    apply_us: observation.apply_us,
                    end_to_end_us: observation.end_to_end_us,
                    outcome: match observation.outcome {
                        ProgramArtifactLaneOutcome::Applied => RuntimeAsyncLaneOutcome::Applied,
                        ProgramArtifactLaneOutcome::StaleRejected => {
                            RuntimeAsyncLaneOutcome::StaleRejected
                        }
                        ProgramArtifactLaneOutcome::Failed => RuntimeAsyncLaneOutcome::Failed,
                    },
                }
            }));
        if drive.poll_required {
            self.next_persistence_poll = Some(Instant::now() + PERSISTENCE_ACK_POLL_INTERVAL);
        }
        Ok((changed, drive.completion))
    }

    fn resolve_program_artifact_requests_blocking(&mut self) -> ViewResult<bool> {
        if self.runtime.is_distributed() {
            return Ok(false);
        }
        let drive = self
            .runtime
            .single_mut()?
            .resolve_program_artifact_requests_blocking(&mut self.program_host, &mut self.sequence)
            .map_err(|error| error.to_string())?;
        self.apply_program_artifact_drive(drive)
            .map(|(changed, _)| changed)
    }

    pub fn resolve_program_artifact_requests(&mut self) -> ViewResult<bool> {
        if self.runtime.is_distributed() {
            return Ok(false);
        }
        let drive = self
            .runtime
            .single_mut()?
            .resolve_program_artifact_requests(&mut self.program_host, &mut self.sequence)
            .map_err(|error| error.to_string())?;
        self.apply_program_artifact_drive(drive)
            .map(|(changed, _)| changed)
    }

    pub fn take_program_requests(&mut self) -> Vec<ProgramHostRequest> {
        match &mut self.runtime {
            RuntimeBackend::Single(runtime) => runtime.take_program_requests(),
            RuntimeBackend::Distributed(_) => Vec::new(),
        }
    }

    pub(crate) fn program_artifact_lane_counts(&self) -> (usize, usize) {
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime.program_artifact_lane_counts(),
            RuntimeBackend::Distributed(_) => (0, 0),
        }
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
            .map(|observed| observed.changed)
    }

    pub(crate) fn complete_program_observed(
        &mut self,
        session: &ProgramSessionId,
        request_id: &ProgramRequestId,
        result: Result<ProgramArtifact, ProgramDiagnostic>,
    ) -> ViewResult<ObservedProgramCompletion> {
        if self.runtime.is_distributed() {
            return Err(
                "nested Program completion has no owner in a distributed Client".to_owned(),
            );
        }
        let drive = self
            .runtime
            .single_mut()?
            .complete_program_observed(
                &mut self.program_host,
                &mut self.sequence,
                session,
                request_id,
                result,
            )
            .map_err(|error| error.to_string())?;
        let (changed, completion) = self.apply_program_artifact_drive(drive)?;
        Ok(ObservedProgramCompletion {
            changed,
            completion: completion.ok_or_else(|| {
                "program completion did not report its final disposition".to_owned()
            })?,
        })
    }

    pub fn poll_program_artifact_stores(&mut self) -> ViewResult<bool> {
        if self.runtime.is_distributed() {
            return Ok(false);
        }
        let drive = self
            .runtime
            .single_mut()?
            .poll_program_artifacts(&mut self.program_host, &mut self.sequence)
            .map_err(|error| error.to_string())?;
        self.apply_program_artifact_drive(drive)
            .map(|(changed, _)| changed)
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
        let value = match &mut self.runtime {
            RuntimeBackend::Single(runtime) => runtime
                .inspect_value_current(path, 8)
                .map_err(|error| error.to_string())?,
            RuntimeBackend::Distributed(runtime) => runtime
                .inspect_client_value_current(path, 8)
                .map_err(|error| error.to_string())?,
        };
        Ok(format_inspection_value(&value, 0))
    }

    #[cfg(test)]
    fn root_value_current(&mut self, path: &str) -> ViewResult<Value> {
        match &mut self.runtime {
            RuntimeBackend::Single(runtime) => runtime
                .root_value_current(path)
                .map_err(|error| error.to_string()),
            RuntimeBackend::Distributed(runtime) => runtime
                .client_root_value_current(path)
                .map_err(|error| error.to_string()),
        }
    }

    #[cfg(test)]
    fn row_target_for_source_text(
        &self,
        path: &str,
        text: &str,
        occurrence: usize,
    ) -> ViewResult<Option<RowId>> {
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime
                .runtime()
                .row_target_for_source_text(path, text, occurrence)
                .map_err(|error| error.to_string()),
            RuntimeBackend::Distributed(runtime) => runtime
                .client_row_target_for_source_text(path, text, occurrence)
                .map_err(|error| error.to_string()),
        }
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
        let row = match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime
                .runtime()
                .row_target_for_source_text(source_path, target_text, occurrence)
                .map_err(|error| error.to_string())?,
            RuntimeBackend::Distributed(runtime) => runtime
                .client_row_target_for_source_text(source_path, target_text, occurrence)
                .map_err(|error| error.to_string())?,
        };
        Ok(row.map(|row| (row.key, row.generation)))
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
                let parent = match &mut self.runtime {
                    RuntimeBackend::Single(runtime) => {
                        runtime
                            .demand_document_window_by_id(
                                materialization,
                                visible,
                                overscan.clone(),
                            )
                            .map_err(|error| error.to_string())?;
                        runtime
                            .runtime()
                            .primary_retained_output_frame()
                            .map_err(|error| error.to_string())?
                            .clone()
                    }
                    RuntimeBackend::Distributed(runtime) => {
                        runtime
                            .demand_client_document_window_by_id(
                                materialization,
                                visible,
                                overscan.clone(),
                            )
                            .map_err(|error| error.to_string())?;
                        runtime.document_frame().cloned().ok_or_else(|| {
                            "distributed Client lost its retained document".to_owned()
                        })?
                    }
                };
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
            && let Some(field) = self.source_row_lookup_field(path).map(str::to_owned)
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
            .or_else(|| self.source_is_row_scoped(path));
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
        match &mut self.runtime {
            RuntimeBackend::Single(runtime) => {
                let event = runtime
                    .runtime()
                    .source_event(next_sequence, path, row, payload)
                    .map_err(|error| error.to_string())?;
                let turn = runtime.dispatch(event).map_err(|error| error.to_string())?;
                self.capture_scenario_turn(path, &turn);
                let source_sequence = turn.source_sequence.ok_or_else(|| {
                    format!("source dispatch `{path}` produced no source sequence")
                })?;
                let changed = self.finish_parent_runtime_turn(turn)?;
                self.record_event_dispatch(path, source_sequence);
                self.schedule_effect_poll()?;
                Ok(changed)
            }
            RuntimeBackend::Distributed(runtime) => {
                runtime
                    .dispatch_client_scoped(path, row, payload)
                    .map_err(|error| error.to_string())?;
                let changed = self.poll_distributed_runtime(Instant::now())?;
                if self.sequence < next_sequence {
                    return Err(format!(
                        "distributed source dispatch `{path}` produced no Client source sequence"
                    ));
                }
                self.record_event_dispatch(path, self.sequence);
                self.schedule_effect_poll()?;
                Ok(changed)
            }
        }
    }

    fn source_row_lookup_field(&self, path: &str) -> Option<&str> {
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime.runtime().source_row_lookup_field(path),
            RuntimeBackend::Distributed(runtime) => runtime.client_source_row_lookup_field(path),
        }
    }

    fn source_is_row_scoped(&self, path: &str) -> Option<bool> {
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime.runtime().source_is_row_scoped(path),
            RuntimeBackend::Distributed(runtime) => runtime.client_source_is_row_scoped(path),
        }
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
        match &self.runtime {
            RuntimeBackend::Single(runtime) => runtime
                .runtime()
                .row_target_for_source_path(source_path, key, generation.unwrap_or(1))
                .map(Some)
                .map_err(|error| error.to_string()),
            RuntimeBackend::Distributed(runtime) => runtime
                .client_row_target_for_source_path(source_path, key, generation.unwrap_or(1))
                .map(Some)
                .map_err(|error| error.to_string()),
        }
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
        match &mut self.runtime {
            RuntimeBackend::Single(runtime) => runtime.queue_program_requests(requests),
            RuntimeBackend::Distributed(_) => {
                debug_assert!(requests.is_empty());
            }
        }
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

    fn dispatch_rejections(&mut self, rejections: Vec<ProgramRejection>) -> ViewResult<bool> {
        let mut changed = false;
        for rejection in rejections {
            let payload = rejected_program_payload(&rejection.diagnostic);
            let paths = self
                .program_host
                .lifecycle_source_paths(&rejection.session, "rejected");
            for path in paths {
                changed |= self.dispatch_source(&path, None, payload.clone())?;
            }
        }
        Ok(changed)
    }

    fn finish_parent_runtime_turn(&mut self, turn: RuntimeTurn) -> ViewResult<bool> {
        self.transient_host.route_turn(&turn)?;
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
            .single()?
            .runtime()
            .primary_retained_output_frame()
            .map_err(|error| error.to_string())?;
        let update = self
            .program_host
            .reconcile_with_parent_patches(parent, parent_patches);
        let mut changed = self.queue_program_update(update.patches, update.requests);
        changed |= self.dispatch_rejections(update.rejections)?;
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
        self.next_effect_poll = match &self.runtime {
            RuntimeBackend::Single(runtime) => {
                let has_work = self.effect_worker.is_busy()
                    || runtime.has_effect_work()
                    || self.transient_host.has_work();
                has_work.then_some(Instant::now() + EFFECT_POLL_INTERVAL)
            }
            RuntimeBackend::Distributed(runtime) => {
                let started = self
                    .distributed_started
                    .ok_or_else(|| "distributed runtime has no logical clock origin".to_owned())?;
                let deadline = runtime.next_deadline().map(|deadline| started + deadline);
                if self.transient_host.has_work() {
                    Some(
                        deadline
                            .unwrap_or_else(Instant::now)
                            .min(Instant::now() + EFFECT_POLL_INTERVAL),
                    )
                } else {
                    deadline
                }
            }
        };
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
        if !has_host_lifecycle_started_source_plan(&self.machine_plan) {
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
    has_host_lifecycle_started_source_plan(runtime.machine_plan())
}

fn has_host_lifecycle_started_source_plan(plan: &MachinePlan) -> bool {
    plan.source_routes
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

fn transient_effect_ids(plan: &MachinePlan) -> impl Iterator<Item = boon_plan::EffectId> + '_ {
    plan.effects.iter().filter_map(|contract| {
        matches!(
            contract.replay,
            EffectReplay::ReadOnly | EffectReplay::ProcessScoped
        )
        .then_some(contract.effect_id)
    })
}

fn scheduled_sources(runtime: &LiveRuntime) -> ViewResult<Vec<ScheduledSource>> {
    scheduled_sources_from_plan(runtime.machine_plan())
}

fn scheduled_sources_from_plan(plan: &MachinePlan) -> ViewResult<Vec<ScheduledSource>> {
    if let Some(source) = plan
        .source_routes
        .iter()
        .find(|source| source.interval_ms == Some(0))
    {
        return Err(format!(
            "scheduled source `{}` has a zero interval",
            source.path
        ));
    }
    let now = Instant::now();
    Ok(plan
        .source_routes
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

fn route_distributed_transient_effects(
    host: &mut NativeTransientHost,
    owners: &mut BTreeMap<boon_runtime::TransientEffectCallId, InProcessTransientEffectOwner>,
    poll: &InProcessPoll,
) -> ViewResult<()> {
    for effect in &poll.transient_effects {
        if owners.contains_key(&effect.invocation.call_id) {
            return Err(format!(
                "distributed runtime repeated active transient call {}",
                effect.invocation.call_id
            ));
        }
    }
    for cancellation in &poll.cancelled_transient_effects {
        if owners.get(&cancellation.call_id) != Some(&cancellation.owner) {
            return Err(format!(
                "distributed runtime cancelled transient call {} through the wrong owner",
                cancellation.call_id
            ));
        }
    }
    for credit in &poll.transient_effect_credit_grants {
        if owners.get(&credit.grant.call_id) != Some(&credit.owner) {
            return Err(format!(
                "distributed runtime granted credit to transient call {} through the wrong owner",
                credit.grant.call_id
            ));
        }
    }
    let cancelled = poll
        .cancelled_transient_effects
        .iter()
        .map(|cancellation| cancellation.call_id)
        .collect::<Vec<_>>();
    let credits = poll
        .transient_effect_credit_grants
        .iter()
        .map(|credit| credit.grant)
        .collect::<Vec<_>>();
    let invocations = poll
        .transient_effects
        .iter()
        .map(|effect| effect.invocation.clone())
        .collect::<Vec<_>>();
    host.route_batch(&cancelled, &credits, &invocations)?;
    for call_id in cancelled {
        owners.remove(&call_id);
    }
    for effect in &poll.transient_effects {
        owners.insert(effect.invocation.call_id, effect.owner);
    }
    Ok(())
}

fn unavailable_persistence_status() -> PersistenceWorkerStatus {
    PersistenceWorkerStatus {
        pending: None,
        checkpoint_batch_in_flight: false,
        queued_checkpoint_batches: 0,
        pending_checkpoint_batches: 0,
        pending_checkpoint_batches_peak: 0,
        durable_epoch: 0,
        durable_through_turn_sequence: 0,
        queue_depth: 0,
        pending_content_artifact_stores: 0,
        pending_content_artifact_loads: 0,
        reserved_slots: 0,
        accepting_turns: false,
        worker_alive: false,
        timings: Default::default(),
        last_error: None,
    }
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

fn transient_content_root(parent: &Path) -> PathBuf {
    parent.join(format!(
        "{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().hyphenated()
    ))
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

pub(crate) fn normalize_key(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "arrowleft" | "leftarrow" => "left".to_owned(),
        "arrowright" | "rightarrow" => "right".to_owned(),
        "arrowup" | "uparrow" => "up".to_owned(),
        "arrowdown" | "downarrow" => "down".to_owned(),
        "prior" | "page_up" | "page up" => "pageup".to_owned(),
        "next" | "page_down" | "page down" => "pagedown".to_owned(),
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
        Value::HostBound { visible, .. } => format_inspection_value(visible, depth),
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
            Value::Number(FiniteReal::new(f64::from(local_x.round())).expect("finite pointer x")),
        );
        payload.fields.insert(
            "pointer_y".to_owned(),
            Value::Number(FiniteReal::new(f64::from(local_y.round())).expect("finite pointer y")),
        );
        payload.fields.insert(
            "pointer_width".to_owned(),
            Value::Number(
                FiniteReal::new(f64::from(target.bounds_width.round()))
                    .expect("finite pointer width"),
            ),
        );
        payload.fields.insert(
            "pointer_height".to_owned(),
            Value::Number(
                FiniteReal::new(f64::from(target.bounds_height.round()))
                    .expect("finite pointer height"),
            ),
        );
    }
    payload
}

fn rejected_program_payload(diagnostic: &ProgramDiagnostic) -> SourcePayload {
    let mut payload = SourcePayload {
        text: Some(diagnostic.message.clone()),
        ..SourcePayload::default()
    };
    for (name, value) in [
        ("revision", diagnostic.revision.to_string()),
        ("source_path", diagnostic.source_path.clone()),
        ("line", diagnostic.line.to_string()),
        ("column", diagnostic.column.to_string()),
        ("diagnostic", diagnostic.message.clone()),
    ] {
        payload.fields.insert(name.to_owned(), Value::Text(value));
    }
    payload
}

fn style_payload_value(value: &StyleValue) -> Option<Value> {
    match value {
        StyleValue::Text(value) => Some(Value::Text(value.clone())),
        StyleValue::Number(value) if value.is_finite() => {
            FiniteReal::new(*value).ok().map(Value::Number)
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::thread;

    fn dispatch_press(view: &mut RuntimeView, source: &str) {
        view.dispatch_source(
            source,
            None,
            SourcePayload {
                fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                ..SourcePayload::default()
            },
        )
        .unwrap();
    }

    fn settle_host_effects(view: &mut RuntimeView, context: &str) {
        let started = Instant::now();
        while view.effect_poll_deadline().is_some() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{context} did not settle"
            );
            view.poll_host_effects(Instant::now() + Duration::from_millis(2))
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            view.transient_host.active_call_count(),
            0,
            "{context} left a native host call active"
        );
    }

    fn assert_real_waveform_reaches_retained_document(
        view: &mut RuntimeView,
        expected_format: &str,
    ) {
        assert_eq!(
            view.root_value_current("store.waveform_format_label")
                .unwrap(),
            Value::Text(expected_format.to_owned())
        );
        assert_eq!(
            view.root_value_current("store.format_label").unwrap(),
            Value::Text("Hex".to_owned()),
            "waveform container format must not replace the selected signal formatter"
        );
        let active_signal = view.root_value_current("store.active_signal").unwrap();
        let Value::Text(active_signal) = active_signal else {
            panic!("real active signal is not text: {active_signal:?}");
        };
        assert!(
            !active_signal.is_empty() && active_signal != "none",
            "{expected_format} hierarchy produced no active signal"
        );
        let requested_signal_ids = view
            .root_value_current("store.real_signal_page_signal_ids")
            .unwrap();
        let signal_page = view
            .root_value_current("store.real_signal_page_result")
            .unwrap();
        let Value::Record(signal_page) = signal_page else {
            panic!("real signal page is not a tagged record: {signal_page:?}");
        };
        let Some(Value::List(signals)) = signal_page.get("signals") else {
            panic!(
                "real signal page has no bounded signal rows for active signal {active_signal:?} \
                 and requested IDs {requested_signal_ids:?}: {signal_page:?}"
            );
        };
        assert_eq!(
            signal_page.get("signal_ids"),
            Some(&Value::List(vec![Value::Text(active_signal.clone())])),
            "{expected_format} page must contain only the selected signal"
        );
        let current_page_fingerprint = view
            .root_value_current("store.real_signal_page_request_fingerprint")
            .unwrap();
        assert_eq!(
            signal_page.get("request_fingerprint"),
            Some(&current_page_fingerprint),
            "{expected_format} page response is stale"
        );
        let transition_count = signals
            .iter()
            .filter_map(|signal| match signal {
                Value::Record(signal) => match signal.get("transitions") {
                    Some(Value::List(transitions)) => Some(transitions.len()),
                    _ => None,
                },
                _ => None,
            })
            .sum::<usize>();
        assert!(
            transition_count > 0,
            "{expected_format} bounded signal page produced no waveform transitions"
        );
        let selected_rows = view
            .root_value_current("store.selected_rows_count")
            .unwrap();
        let Value::Number(selected_rows) = selected_rows else {
            panic!("real selected row count is not a Number: {selected_rows:?}");
        };
        assert!(
            selected_rows.to_i64_exact().unwrap() > 0,
            "{expected_format} real hierarchy produced no retained signal rows"
        );

        let cursor_values = view
            .root_value_current("store.real_cursor_values_result")
            .unwrap();
        let Value::Record(cursor_values) = cursor_values else {
            panic!("real cursor response is not a tagged record: {cursor_values:?}");
        };
        assert_eq!(
            cursor_values.get("$tag"),
            Some(&Value::Text("CursorValues".to_owned())),
            "{expected_format} cursor request failed: {cursor_values:?}"
        );
        let current_cursor_fingerprint = view
            .root_value_current("store.real_cursor_request_fingerprint")
            .unwrap();
        assert_eq!(
            cursor_values.get("request_fingerprint"),
            Some(&current_cursor_fingerprint),
            "{expected_format} cursor response is stale"
        );
        let current_cursor_time = view
            .root_value_current("store.real_cursor_time_tick")
            .unwrap();
        assert_eq!(
            cursor_values.get("cursor_time"),
            Some(&current_cursor_time),
            "{expected_format} cursor response used the wrong timeline position"
        );

        let hierarchy = view
            .root_value_current("store.real_hierarchy_page_result")
            .unwrap();
        let Value::Record(hierarchy) = hierarchy else {
            panic!("real hierarchy page is not a tagged record: {hierarchy:?}");
        };
        let Some(Value::List(hierarchy_rows)) = hierarchy.get("rows") else {
            panic!("real hierarchy page has no rows: {hierarchy:?}");
        };
        let active_signal_name = hierarchy_rows.iter().find_map(|row| {
            let Value::Record(row) = row else {
                return None;
            };
            (row.get("signal_id") == Some(&Value::Text(active_signal.clone())))
                .then(|| row.get("name"))
                .flatten()
                .and_then(|name| match name {
                    Value::Text(name) => Some(name.as_str()),
                    _ => None,
                })
        });
        let active_signal_name = active_signal_name
            .unwrap_or_else(|| panic!("{expected_format} active signal is absent from hierarchy"));

        let visible_text = view
            .retained_frame()
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
            .collect::<BTreeSet<_>>();
        assert!(
            visible_text
                .iter()
                .any(|text| text.contains(expected_format)),
            "{expected_format} container format did not reach retained UI text"
        );
        assert!(
            visible_text.contains(active_signal_name),
            "{expected_format} real active signal did not reach retained UI text"
        );
    }

    #[test]
    fn distributed_package_mounts_and_dispatches_through_one_aggregate() {
        let bundle = crate::distributed_program::compile_distributed_program(
            crate::distributed_program::distributed_fixture_sources(),
        )
        .expect("compile distributed fixture");
        let mut view = RuntimeView::open_distributed_with_assets(bundle, true, &[])
            .expect("mount distributed fixture");

        assert!(view.runtime.is_distributed());
        assert!(matches!(
            view.startup_evidence().disposition,
            PersistentRuntimeStartupDisposition::Fresh
        ));
        assert!(view.persistence_status().worker_alive);
        assert!(view.persistence_status().accepting_turns);
        assert_eq!(
            view.root_value_current("store.client_count").unwrap(),
            Value::integer(0).unwrap()
        );
        assert!(view.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text == "Distributed program fixture")
        }));

        view.dispatch_source("store.increment", None, SourcePayload::default())
            .expect("dispatch Client event through distributed aggregate");
        assert_eq!(
            view.root_value_current("store.client_count").unwrap(),
            Value::integer(1).unwrap()
        );
    }

    #[test]
    fn distributed_client_effect_completes_through_native_exact_call_host() {
        let mut sources = crate::distributed_program::distributed_fixture_sources();
        let client = sources
            .iter_mut()
            .find(|source| source.role == ProgramRole::Client)
            .expect("distributed Client source");
        client
            .units
            .iter_mut()
            .find(|unit| unit.path.ends_with("Client/RUN.bn"))
            .expect("distributed Client entry unit")
            .source = r#"
store: [
    increment: SOURCE
    randomize: SOURCE
    random:
        RandomNotRead |> HOLD random {
            randomize |> THEN { Random/bytes(byte_count: 1) }
        }
    random_size:
        random |> WHEN {
            RandomBytesReady => random.bytes |> Bytes/length()
            __ => 0
        }
]

document: Document/new(
    root: Element/label(
        element: []
        label: DistributedContract/client_label()
    )
)
"#
        .to_owned();
        let bundle = crate::distributed_program::compile_distributed_program(sources)
            .expect("compile distributed Client effect fixture");
        let mut view = RuntimeView::open_distributed_with_assets(bundle, true, &[])
            .expect("mount distributed Client effect fixture");

        view.dispatch_source("store.randomize", None, SourcePayload::default())
            .expect("dispatch Client random effect");
        let started = Instant::now();
        while view.effect_poll_deadline().is_some() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "native Client effect did not settle"
            );
            view.poll_host_effects(Instant::now() + Duration::from_millis(2))
                .expect("poll native Client effect host");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            view.root_value_current("store.random_size").unwrap(),
            Value::integer(1).unwrap()
        );
    }

    #[test]
    fn native_package_stream_enforces_four_credit_backpressure_and_cleans_up() {
        let source = r#"
store: [
    read: SOURCE
    asset: PackageAsset[url: TEXT { asset://fixture/bounded.bin }]
    stream_result:
        NotStarted |> HOLD stream_result {
            read |> THEN {
                File/read_stream(
                    file: asset
                    chunk_bytes: 65536
                    retain_content: True
                )
            }
        }
    byte_count:
        stream_result |> WHEN {
            Finished => stream_result.byte_count
            __ => 0
        }
    byte_count_label: byte_count |> Number/to_text(radix: 10)
]

document: Document/new(
    root: Element/label(element: [], label: store.byte_count_label)
)
"#;
        let runtime = LiveRuntime::from_source("native-stream-backpressure.bn", source).unwrap();
        let bytes = vec![0x5a; 5 * 65536 + 17];
        let asset = AssetBlob {
            url: "asset://fixture/bounded.bin".to_owned(),
            media_type: "application/octet-stream".to_owned(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes: bytes.clone(),
        };
        let mut view =
            RuntimeView::open_for_scenario_with_assets(runtime.shared_machine_plan(), &[asset])
                .unwrap();

        view.dispatch_source("store.read", None, SourcePayload::default())
            .unwrap();
        assert_eq!(view.transient_host.active_call_count(), 1);
        let started = Instant::now();
        loop {
            let credits = view.transient_host.file_stream_outstanding_credits();
            assert_eq!(credits.len(), 1);
            assert!(credits[0] <= 4, "stream exceeded its four-credit bound");
            if credits[0] == 0 {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "native package stream did not consume its initial bounded credits"
            );
            thread::sleep(Duration::from_millis(1));
        }

        settle_host_effects(&mut view, "native package stream backpressure");
        assert_eq!(
            view.root_value_current("store.byte_count").unwrap(),
            Value::integer(bytes.len() as i64).unwrap()
        );
        assert_eq!(view.transient_host.file_stream_owned_call_count(), 0);
        assert_eq!(view.transient_host.pending_content_writer_count(), 0);
    }

    #[test]
    fn novywave_package_asset_runs_through_file_and_wellen_effect_chain() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| boon_runtime::RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
        let mut view = RuntimeView::open_for_scenario_with_assets(
            runtime.shared_machine_plan(),
            &example.assets,
        )
        .unwrap();
        dispatch_press(&mut view, "store.elements.load_default_file");
        assert_eq!(view.transient_host.active_call_count(), 1);
        dispatch_press(&mut view, "store.elements.show_empty");
        assert_eq!(
            view.transient_host.active_call_count(),
            0,
            "leaving NovyWave's active WHILE branch must cancel the file stream immediately"
        );
        dispatch_press(&mut view, "store.elements.load_default_file");
        assert_eq!(view.transient_host.active_call_count(), 1);

        dispatch_press(&mut view, "store.elements.select_uart_compare_file");
        assert_eq!(
            view.transient_host.active_call_count(),
            2,
            "comparison mode must own exactly one FST stream and one VCD reference stream"
        );
        dispatch_press(&mut view, "store.elements.select_ghw_file");
        assert_eq!(
            view.transient_host.active_call_count(),
            1,
            "replacing FST with GHW must keep only the newest file stream"
        );
        settle_host_effects(&mut view, "rapid NovyWave VCD/FST/GHW replacement");
        assert_real_waveform_reaches_retained_document(&mut view, "GHW");

        for (path, expected_tag) in [
            ("store.real_file_stream_result", "Finished"),
            ("store.real_waveform_open_result", "WaveformOpened"),
            ("store.real_hierarchy_page_result", "HierarchyPage"),
            ("store.real_signal_page_result", "SignalPage"),
            ("store.real_cursor_values_result", "CursorValues"),
        ] {
            let value = view.root_value_current(path).unwrap();
            assert!(
                matches!(&value,
                    Value::Record(fields)
                        if fields.get("$tag") == Some(&Value::Text(expected_tag.to_owned()))),
                "{path} did not reach {expected_tag}: {value:?}"
            );
        }

        let hierarchy = view
            .root_value_current("store.bridge_hierarchy_page_label")
            .unwrap();
        let Value::Text(hierarchy) = hierarchy else {
            panic!("visible hierarchy status is not text: {hierarchy:?}");
        };
        for expected in ["hierarchy rows", "signal pages", "cursor values"] {
            assert!(
                hierarchy.contains(expected),
                "visible hierarchy status omitted `{expected}`: {hierarchy}"
            );
        }
        let visible_text = view
            .retained_frame()
            .nodes
            .values()
            .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
            .collect::<Vec<_>>();
        assert!(visible_text.iter().any(|text| text.contains("GHW")));

        for (file, expected_format) in [
            ("simple.vcd", "VCD"),
            ("wave_27.fst", "FST"),
            ("simple_test.ghw", "GHW"),
        ] {
            let target = view
                .row_target_for_source_text("file_tree_row.file_row_elements.select_file", file, 0)
                .unwrap()
                .unwrap();
            view.dispatch_source(
                "file_tree_row.file_row_elements.select_file",
                Some(target),
                SourcePayload {
                    address: Some(file.to_owned()),
                    fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                    ..SourcePayload::default()
                },
            )
            .unwrap();
            if expected_format == "FST" {
                assert_eq!(
                    view.root_value_current("store.comparison_waveform_mode")
                        .unwrap(),
                    Value::Text("Active".to_owned()),
                    "the FST row must enter comparison mode"
                );
                assert_eq!(
                    view.transient_host.active_call_count(),
                    2,
                    "the FST row must start one primary and one comparison stream"
                );
            }
            settle_host_effects(
                &mut view,
                &format!("real NovyWave host-effect replacement for {file}"),
            );
            assert_real_waveform_reaches_retained_document(&mut view, expected_format);
            if expected_format == "FST" {
                for (path, expected_tag) in [
                    ("store.comparison_file_stream_result", "Finished"),
                    ("store.comparison_waveform_open_result", "WaveformOpened"),
                    ("store.comparison_hierarchy_page_result", "HierarchyPage"),
                    ("store.comparison_signal_page_result", "SignalPage"),
                    ("store.comparison_cursor_values_result", "CursorValues"),
                ] {
                    let value = view.root_value_current(path).unwrap();
                    assert!(
                        matches!(&value,
                            Value::Record(fields)
                                if fields.get("$tag")
                                    == Some(&Value::Text(expected_tag.to_owned()))),
                        "{path} did not reach {expected_tag}: {value:?}"
                    );
                }
                assert_eq!(
                    view.root_value_current("store.compare_file").unwrap(),
                    Value::Text("simple.vcd".to_owned())
                );
                let comparison_visible =
                    view.root_value_current("store.comparison_visible").unwrap();
                let comparison_first_signal_id = view
                    .root_value_current("store.comparison_first_signal_id")
                    .unwrap();
                let comparison_signal_name = view
                    .root_value_current("store.comparison_signal_name")
                    .unwrap();
                let reference_texts = view
                    .retained_frame()
                    .nodes
                    .values()
                    .filter_map(|node| node.text.as_ref().map(|text| text.text.as_str()))
                    .filter(|text| text.contains("Reference"))
                    .collect::<Vec<_>>();
                assert!(
                    view.retained_frame().nodes.values().any(|node| {
                        node.text
                            .as_ref()
                            .is_some_and(|text| text.text.contains("Reference: simple_tb.s.A"))
                    }),
                    "retained NovyWave frame omitted the real comparison lane: visible={comparison_visible:?}, first_signal={comparison_first_signal_id:?}, signal_name={comparison_signal_name:?}, reference_texts={reference_texts:?}"
                );
            } else {
                assert_eq!(
                    view.root_value_current("store.compare_file").unwrap(),
                    Value::Text("none".to_owned()),
                    "leaving comparison mode must clear the retained reference artifact"
                );
            }
        }

        let vcd_target = view
            .row_target_for_source_text(
                "file_tree_row.file_row_elements.select_file",
                "simple.vcd",
                0,
            )
            .unwrap()
            .unwrap();
        view.dispatch_source(
            "file_tree_row.file_row_elements.select_file",
            Some(vcd_target),
            SourcePayload {
                address: Some("simple.vcd".to_owned()),
                fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                ..SourcePayload::default()
            },
        )
        .unwrap();
        settle_host_effects(&mut view, "real VCD analog selection setup");

        let hierarchy = view
            .root_value_current("store.real_hierarchy_page_result")
            .unwrap();
        let Value::Record(hierarchy) = hierarchy else {
            panic!("real VCD hierarchy is not a record: {hierarchy:?}");
        };
        let Some(Value::List(hierarchy_rows)) = hierarchy.get("rows") else {
            panic!("real VCD hierarchy has no rows: {hierarchy:?}");
        };
        let (analog_signal_id, analog_signal_name) = hierarchy_rows
            .iter()
            .find_map(|row| {
                let Value::Record(row) = row else {
                    return None;
                };
                if row.get("encoding") != Some(&Value::Text("Real".to_owned())) {
                    return None;
                }
                let (Some(Value::Text(signal_id)), Some(Value::Text(name))) =
                    (row.get("signal_id"), row.get("name"))
                else {
                    return None;
                };
                Some((signal_id.clone(), name.clone()))
            })
            .expect("committed VCD must expose a real-valued signal");
        let analog_target = view
            .row_target_for_source_text(
                "signal_row.signal_elements.select_signal",
                &analog_signal_id,
                0,
            )
            .unwrap()
            .unwrap();
        view.dispatch_source(
            "signal_row.signal_elements.select_signal",
            Some(analog_target),
            SourcePayload {
                fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                ..SourcePayload::default()
            },
        )
        .unwrap();
        settle_host_effects(&mut view, "real VCD analog signal page");
        assert_eq!(
            view.root_value_current("store.active_signal").unwrap(),
            Value::Text(analog_signal_id.clone())
        );
        let analog_page = view
            .root_value_current("store.real_signal_page_result")
            .unwrap();
        let Value::Record(analog_page) = &analog_page else {
            panic!("real analog signal page is not a record: {analog_page:?}");
        };
        let has_real_value = analog_page
            .get("signals")
            .and_then(|signals| match signals {
                Value::List(signals) => Some(signals),
                _ => None,
            })
            .into_iter()
            .flatten()
            .filter_map(|signal| match signal {
                Value::Record(signal) => signal.get("transitions"),
                _ => None,
            })
            .filter_map(|transitions| match transitions {
                Value::List(transitions) => Some(transitions),
                _ => None,
            })
            .flatten()
            .any(|transition| {
                matches!(transition,
                    Value::Record(transition)
                        if matches!(transition.get("value"),
                            Some(Value::Record(value))
                                if value.get("$tag")
                                    == Some(&Value::Text("RealValue".to_owned()))))
            });
        assert!(has_real_value, "real VCD analog page contains no RealValue");
        assert!(view.retained_frame().nodes.values().any(|node| {
            node.text
                .as_ref()
                .is_some_and(|text| text.text.contains(&analog_signal_name))
        }));
        assert_eq!(analog_page.get("has_more"), Some(&Value::Bool(true)));
        let first_analog_page_fingerprint = analog_page
            .get("request_fingerprint")
            .cloned()
            .expect("first analog page fingerprint");
        dispatch_press(&mut view, "store.elements.signal_page_next");
        settle_host_effects(&mut view, "real VCD analog continuation page");
        assert_eq!(
            view.root_value_current("store.real_signal_page_offset")
                .unwrap(),
            Value::integer(2).unwrap()
        );
        let next_analog_page = view
            .root_value_current("store.real_signal_page_result")
            .unwrap();
        let Value::Record(next_analog_page) = next_analog_page else {
            panic!("continued analog page is not a record: {next_analog_page:?}");
        };
        assert_eq!(
            next_analog_page.get("offset"),
            Some(&Value::integer(2).unwrap())
        );
        assert_ne!(
            next_analog_page.get("request_fingerprint"),
            Some(&first_analog_page_fingerprint)
        );

        let initial_page_fingerprint = view
            .root_value_current("store.real_signal_page_request_fingerprint")
            .unwrap();
        dispatch_press(&mut view, "store.elements.zoom_in");
        settle_host_effects(&mut view, "real VCD zoom signal page replacement");
        let zoom_page_fingerprint = view
            .root_value_current("store.real_signal_page_request_fingerprint")
            .unwrap();
        assert_ne!(zoom_page_fingerprint, initial_page_fingerprint);
        let page_before_cursor = view
            .root_value_current("store.real_signal_page_result")
            .unwrap();
        let cursor_before = view
            .root_value_current("store.real_cursor_request_fingerprint")
            .unwrap();
        dispatch_press(&mut view, "store.elements.cursor_right");
        settle_host_effects(&mut view, "real VCD cursor-only replacement");
        assert_eq!(
            view.root_value_current("store.real_signal_page_result")
                .unwrap(),
            page_before_cursor,
            "cursor movement must not replace the retained signal page"
        );
        assert_ne!(
            view.root_value_current("store.real_cursor_request_fingerprint")
                .unwrap(),
            cursor_before,
            "cursor movement must replace the cursor request"
        );
        dispatch_press(&mut view, "store.elements.pan_right");
        settle_host_effects(&mut view, "real VCD pan signal page replacement");
        assert_ne!(
            view.root_value_current("store.real_signal_page_request_fingerprint")
                .unwrap(),
            zoom_page_fingerprint,
            "pan must replace the bounded signal page request"
        );
        assert_eq!(view.transient_host.active_call_count(), 0);

        dispatch_press(&mut view, "store.elements.show_empty");
        view.root_value_current("store.external_file_tree_selected_file")
            .expect("empty-state list membership change must remain a current non-event value");
    }

    #[test]
    fn novywave_semantic_scenario_runs_through_real_host_effects() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("novywave")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| boon_runtime::RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/novywave/RUN.bn", &units).unwrap();
        let mut view = RuntimeView::open_for_scenario_with_assets(
            runtime.shared_machine_plan(),
            &example.assets,
        )
        .unwrap();
        let scenario_path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/novywave.scn");
        let scenario = boon_runtime::parse_scenario(&scenario_path).unwrap();
        let mut failures = Vec::new();

        for step in &scenario.steps {
            if let Some(event) = &step.source_event {
                view.begin_scenario_step(&event.source);
                let target_text = event
                    .target_text
                    .as_deref()
                    .or(event.payload.address.as_deref());
                let row_result = if let Some(target_text) = target_text {
                    view.row_target_for_source_text(
                        &event.source,
                        target_text,
                        event.target_occurrence.unwrap_or(0),
                    )
                } else if let Some(key) = event.target_key {
                    view.row_target(&event.source, Some(key), event.target_generation)
                } else {
                    Ok(None)
                };
                let row = match row_result {
                    Ok(row) => row,
                    Err(error) => {
                        failures.push(format!("{} target: {error}", step.id));
                        continue;
                    }
                };
                if let Err(error) = view.dispatch_source(&event.source, row, event.payload.clone())
                {
                    failures.push(format!("{} dispatch: {error}", step.id));
                    continue;
                }
            }
            settle_host_effects(&mut view, &step.id);
            if let Err(error) = view.assert_scenario_step(step) {
                let error = error.to_string();
                let boundary = error
                    .char_indices()
                    .nth(1200)
                    .map_or(error.len(), |(index, _)| index);
                failures.push(error[..boundary].to_owned());
            }
        }

        assert!(
            failures.is_empty(),
            "NovyWave semantic scenario failures ({}):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    #[test]
    fn persons_passkey_cancellation_settles_without_changing_anonymous_authority() {
        let example = crate::catalog::Catalog::load()
            .unwrap()
            .open("persons_pro")
            .unwrap();
        let units = example
            .units
            .iter()
            .map(|unit| boon_runtime::RuntimeSourceUnit {
                path: unit.path.clone(),
                source: unit.source.clone(),
            })
            .collect::<Vec<_>>();
        let runtime = LiveRuntime::from_project("examples/persons_pro/RUN.bn", &units).unwrap();
        let mut view = RuntimeView::open_for_scenario_with_assets(
            runtime.shared_machine_plan(),
            &example.assets,
        )
        .unwrap();
        assert_eq!(
            view.root_value_current("store.credential_count").unwrap(),
            Value::Number(FiniteReal::from_i64_exact(0).unwrap())
        );
        view.dispatch_source(
            "store.elements.simulate_registration_cancel",
            None,
            SourcePayload {
                fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                ..SourcePayload::default()
            },
        )
        .unwrap();

        let started = Instant::now();
        while view.effect_poll_deadline().is_some() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "Persons.pro passkey cancellation did not settle"
            );
            view.poll_host_effects(Instant::now() + Duration::from_millis(2))
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            view.root_value_current("store.credential_count").unwrap(),
            Value::Number(FiniteReal::from_i64_exact(0).unwrap()),
            "cancelled registration must not append a credential"
        );
        for (path, expected) in [
            ("store.passkey_workflow_state", "Cancelled"),
            ("store.workspace_grant_state", "AnonymousGrant"),
            ("store.account_state", "Anonymous"),
        ] {
            assert_eq!(
                view.root_value_current(path).unwrap(),
                Value::Text(expected.to_owned()),
                "passkey cancellation changed `{path}` incorrectly"
            );
        }

        view.dispatch_source(
            "store.elements.register_passkey",
            None,
            SourcePayload {
                fields: BTreeMap::from([("press".to_owned(), Value::Bool(true))]),
                ..SourcePayload::default()
            },
        )
        .unwrap();
        let started = Instant::now();
        while view.effect_poll_deadline().is_some() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "Persons.pro passkey registration did not settle"
            );
            view.poll_host_effects(Instant::now() + Duration::from_millis(2))
                .unwrap();
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            view.root_value_current("store.credential_count").unwrap(),
            Value::Number(FiniteReal::from_i64_exact(1).unwrap()),
            "one successful registration must append exactly one credential"
        );
        for (path, expected) in [
            ("store.passkey_workflow_state", "Registered"),
            ("store.workspace_grant_state", "PendingRevocation"),
            ("store.account_state", "OnePasskey"),
        ] {
            assert_eq!(
                view.root_value_current(path).unwrap(),
                Value::Text(expected.to_owned()),
                "passkey registration did not update `{path}`"
            );
        }
    }
}
