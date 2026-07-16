use std::collections::BTreeMap;
use std::path::Path;
use std::thread;

use boon_document::diff_document_frames;
use boon_editor::Position;
use boon_host::{HostEvent, HostEventEnvelope, HostEventOrigin, PointerPhase, Viewport};
use boon_native_app_window::{NativeRoleResult, NativeSurfaceHost};
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};

use crate::dev_state::{ClipboardAction, DevAction, DevChange, DevState, NameEditTarget};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, host_event_digest,
    input_kind, pointer_button_pressed,
};
use crate::language::{LanguageSnapshot, LanguageWorker};
use crate::observer::{InputAccepted, ObserverClient, ObserverEvent, ObserverRole};
use crate::protocol::{
    ApplicationIdentity, AuthoritySelection, CanonicalStateArtifact, CatalogItem, Connection,
    FrameMode, Message, MigrationBundle, MigrationCommand, MigrationOperation, MigrationStatus,
    PersistenceCommand, PersistenceSnapshot, PreviewStats, ProofMode, Role, SourceUnit,
};
use crate::ui::{
    DEV_EDITOR, DEV_EDITOR_INPUT_TARGET, DEV_FILE_NEW, DEV_FILE_REMOVE, DEV_FILE_RENAME,
    DEV_FORMAT, DEV_NEW, DEV_NEXT, DEV_PREVIOUS, DEV_REMOVE, DEV_RENAME, DEV_RENAME_CANCEL,
    DEV_RENAME_INPUT, DEV_RENAME_SAVE, DEV_RESET, DEV_RUN, DEV_SAVE, DEV_TEST, DevFrameState,
    InspectorMode, InspectorState, MigrationUiState, OUTBOX_WINDOW_ROWS, PersistenceUiState,
    dev_frame, editor_first_line, editor_line_from_target,
};
use crate::view::RetainedView;
use crate::workspace::{
    PersistRequest, PersistenceWorker, ProjectOrigin, ProjectStore, StoredProject,
};

pub fn connect(path: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Connection::connect(path, Role::Dev)?)
}

struct DevModel {
    catalog: Vec<CatalogItem>,
    custom: BTreeMap<String, StoredProject>,
    active: StoredProject,
    active_file: usize,
    revision: u64,
    request_id: u64,
    inspect_request_id: u64,
    pending_inspection: Option<(u64, u64, String)>,
    runtime_sequence: Option<(u64, u64)>,
    runtime_value_path: Option<(u64, u64, String)>,
    language: Option<LanguageSnapshot>,
    source_publication_revision: Option<u64>,
    migration: Option<MigrationBundle>,
    active_migration_stage: Option<String>,
    selected_migration_stage: Option<String>,
    migration_status: Option<MigrationStatus>,
    start_over_armed: bool,
    persistence_snapshot: Option<PersistenceSnapshot>,
    inspector_mode: InspectorMode,
    outbox_offset: usize,
    clear_all_armed: bool,
    clear_selected_armed: bool,
    selected_authority: Option<AuthoritySelection>,
    state_artifact: Option<CanonicalStateArtifact>,
    perf: String,
    runtime_value: String,
}

impl DevModel {
    fn waiting() -> Self {
        Self {
            catalog: Vec::new(),
            custom: BTreeMap::new(),
            active: StoredProject {
                id: "waiting".to_owned(),
                label: "Boon".to_owned(),
                origin: ProjectOrigin::BuiltIn,
                application: ApplicationIdentity::compiler_default(),
                units: vec![SourceUnit {
                    path: "no source".to_owned(),
                    source: String::new(),
                }],
            },
            active_file: 0,
            revision: 0,
            request_id: 0,
            inspect_request_id: 0,
            pending_inspection: None,
            runtime_sequence: None,
            runtime_value_path: None,
            language: None,
            source_publication_revision: None,
            migration: None,
            active_migration_stage: None,
            selected_migration_stage: None,
            migration_status: None,
            start_over_armed: false,
            persistence_snapshot: None,
            inspector_mode: InspectorMode::Value,
            outbox_offset: 0,
            clear_all_armed: false,
            clear_selected_armed: false,
            selected_authority: None,
            state_artifact: None,
            perf: "Preview idle, state waiting, proof off".to_owned(),
            runtime_value: "Waiting for preview".to_owned(),
        }
    }

    fn sync_editor(&mut self, state: &DevState) {
        if let Some(unit) = self.active.units.get_mut(self.active_file) {
            unit.source = state.source();
        }
        if self.active.origin == ProjectOrigin::Custom {
            self.custom
                .insert(self.active.id.clone(), self.active.clone());
        }
    }

    fn take_source_publication(&mut self, revision: u64) -> bool {
        if self.source_publication_revision == Some(revision) {
            self.source_publication_revision = None;
            true
        } else {
            false
        }
    }

    fn source_paths(&self) -> Vec<String> {
        self.active
            .units
            .iter()
            .map(|unit| unit.path.clone())
            .collect()
    }

    fn source(&self) -> String {
        self.active
            .units
            .get(self.active_file)
            .map_or_else(String::new, |unit| unit.source.clone())
    }

    fn set_catalog(&mut self, built_ins: Vec<CatalogItem>) {
        self.catalog = built_ins;
        self.catalog
            .extend(self.custom.values().map(|project| CatalogItem {
                id: project.id.clone(),
                label: project.label.clone(),
                custom: true,
            }));
    }

    fn upsert_custom_catalog(&mut self, project: &StoredProject) {
        if let Some(entry) = self.catalog.iter_mut().find(|entry| entry.id == project.id) {
            entry.label.clone_from(&project.label);
            entry.custom = true;
        } else {
            self.catalog.push(CatalogItem {
                id: project.id.clone(),
                label: project.label.clone(),
                custom: true,
            });
        }
    }

    fn install_migration(
        &mut self,
        migration: Option<MigrationBundle>,
        active_stage: Option<String>,
    ) {
        self.migration = migration;
        self.active_migration_stage = active_stage;
        self.selected_migration_stage = self
            .migration
            .as_ref()
            .zip(self.active_migration_stage.as_deref())
            .and_then(|(migration, active)| next_migration_stage(migration, active));
        self.migration_status = None;
        self.start_over_armed = false;
    }

    fn migration_status_text(&self) -> String {
        self.migration_status.as_ref().map_or_else(
            || {
                self.active_migration_stage.as_ref().map_or_else(
                    || "No migration sequence".to_owned(),
                    |stage| format!("Active {stage}; select a forward target"),
                )
            },
            |status| status.message.clone(),
        )
    }

    fn clear_persistence_cache(&mut self) {
        self.persistence_snapshot = None;
        self.outbox_offset = 0;
        self.clear_all_armed = false;
        self.clear_selected_armed = false;
        self.selected_authority = None;
        self.state_artifact = None;
    }

    fn apply_persistence_snapshot(&mut self, snapshot: PersistenceSnapshot) -> bool {
        if snapshot.revision != self.revision
            || snapshot.application != self.active.application
            || self
                .persistence_snapshot
                .as_ref()
                .is_some_and(|current| current.snapshot_sequence >= snapshot.snapshot_sequence)
        {
            return false;
        }
        let sample_count = snapshot.outbox.samples.len();
        self.outbox_offset = self
            .outbox_offset
            .min(sample_count.saturating_sub(OUTBOX_WINDOW_ROWS));
        self.clear_all_armed = false;
        self.clear_selected_armed = false;
        self.persistence_snapshot = Some(snapshot);
        true
    }
}

struct InspectorOwned {
    symbol: String,
    static_type: String,
    detail: String,
    current_value: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InspectionResultDisposition {
    Ignored,
    Applied,
    Retry,
}

pub async fn run(mut host: NativeSurfaceHost, mut writer: Connection) -> NativeRoleResult {
    let observer = ObserverClient::from_env()?;
    let mut product = ProductFrame::attach(&mut host, ObserverRole::Dev, false).await?;
    emit(
        &observer,
        ObserverEvent::RoleMetadata(product.role_metadata()),
    );

    let store = ProjectStore::discover()?;
    let custom_projects = store
        .load_custom()?
        .into_iter()
        .map(|project| (project.id.clone(), project))
        .collect::<BTreeMap<_, _>>();
    let persistence = PersistenceWorker::new()?;
    let mut language_worker = LanguageWorker::new();
    let mut language_output = language_worker.take_output();
    let mut model = DevModel::waiting();
    model.custom = custom_projects;
    let mut state = DevState::new(String::new());
    state.set_status("Waiting for source...");

    let mut columns = boon_native_gpu::GlyphonRenderTextColumnMeasurer::new();
    let initial = build_frame(&model, &state);
    let mut view = RetainedView::new(initial, viewport(&host), &mut columns)?;
    if let Some(presented) = product.present(&mut host, &view).await? {
        emit_presented(&observer, &presented);
    }

    let (incoming_tx, mut incoming) = mpsc::unbounded::<Result<Message, String>>();
    let mut reader = writer.try_clone()?;
    thread::Builder::new()
        .name("boon-dev-ipc".to_owned())
        .spawn(move || {
            loop {
                let item = match reader.receive() {
                    Ok(Some(message)) => Ok(message),
                    Ok(None) => Err("desktop IPC closed".to_owned()),
                    Err(error) => Err(error.to_string()),
                };
                let closed = item.is_err();
                if incoming_tx.unbounded_send(item).is_err() || closed {
                    break;
                }
            }
        })?;
    writer.send(&Message::Ready { role: Role::Dev })?;

    let mut clipboard = None::<arboard::Clipboard>;
    let mut observed_targets = BTreeMap::<String, (f32, f32)>::new();
    loop {
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
            Language(Option<LanguageSnapshot>),
        }
        let wake = {
            let native = host.next_event().fuse();
            let command = incoming.next().fuse();
            let language = language_output.next().fuse();
            pin_mut!(native, command, language);
            select! {
                event = native => Wake::Native(event),
                message = command => Wake::Ipc(message),
                snapshot = language => Wake::Language(snapshot),
            }
        };
        let mut transaction = NativeFrameTransaction::default();
        let mut frame_changed = false;
        let mut interaction_changed = false;
        match wake {
            Wake::Native(event) => {
                for accepted in drain_native_events(&mut host, event).await? {
                    let envelope = &accepted.envelope;
                    let target_name = native_target_name(&view, &envelope.event, &state);
                    let visible_input = if let HostEvent::Resize(resize) = &envelope.event {
                        view.resize(viewport(&host), &mut columns)?;
                        emit(
                            &observer,
                            ObserverEvent::RoleMetadata(
                                product.current_role_metadata(&host, resize.epoch),
                            ),
                        );
                        frame_changed = true;
                        true
                    } else {
                        let result = state.handle_event(&envelope.event, |x, y| {
                            view.hit(x, y).map(str::to_owned)
                        });
                        interaction_changed |= result.change == DevChange::Interaction;
                        frame_changed |= matches!(
                            result.change,
                            DevChange::EditorText
                                | DevChange::EditorSelection
                                | DevChange::Rename
                                | DevChange::Scroll
                        );
                        if result.change == DevChange::EditorText {
                            model.sync_editor(&state);
                            model.revision = model.revision.saturating_add(1);
                            model.source_publication_revision = Some(model.revision);
                            language_worker.submit(
                                model.revision,
                                model.active_file,
                                model.active.units.clone(),
                            );
                        } else if result.change == DevChange::EditorSelection {
                            request_inspection(&mut model, &state, &mut writer)?;
                        }
                        let mut inspector_changed = false;
                        if let HostEvent::Pointer(pointer) = &envelope.event
                            && matches!(pointer.phase, PointerPhase::Down | PointerPhase::Move)
                        {
                            let dragging = state.dragging_editor();
                            let target = view.hit(pointer.x, pointer.y).map(str::to_owned);
                            if pointer.phase == PointerPhase::Move && !dragging {
                                let position = editor_slot_at(&view, target.as_deref(), pointer.y)
                                    .map(|slot| {
                                        let line = editor_first_line(state.editor_scroll()) + slot;
                                        let column = editor_column(&view, &state, line, pointer.x);
                                        Position { line, column }
                                    });
                                if state.set_inspector_position(position) {
                                    inspector_changed = true;
                                    frame_changed = true;
                                    request_inspection(&mut model, &state, &mut writer)?;
                                }
                            } else if let Some(slot) =
                                editor_slot_at(&view, target.as_deref(), pointer.y)
                                && (pointer.phase == PointerPhase::Down || dragging)
                            {
                                let line = editor_first_line(state.editor_scroll()) + slot;
                                let column = editor_column(&view, &state, line, pointer.x);
                                let caret_changed = state.set_caret(
                                    Position { line, column },
                                    pointer.phase == PointerPhase::Move || state.shift_held(),
                                );
                                frame_changed |= caret_changed;
                                if caret_changed {
                                    request_inspection(&mut model, &state, &mut writer)?;
                                }
                                inspector_changed |= caret_changed;
                            }
                        }
                        let changed = result.visible_change() || inspector_changed;
                        let action_changed = result.action != DevAction::None;
                        let close = result.action == DevAction::Close;
                        handle_action(
                            result.action,
                            &mut model,
                            &mut state,
                            &store,
                            &persistence,
                            &language_worker,
                            &mut writer,
                            &mut clipboard,
                        )?;
                        frame_changed |= action_changed;
                        if close {
                            return Ok(());
                        }
                        changed
                    };
                    observe_input(&observer, envelope, target_name, visible_input);
                    if visible_input {
                        transaction.visible_change(&accepted);
                    }
                }
            }
            Wake::Ipc(message) => {
                let message = message.ok_or("desktop IPC reader stopped")??;
                match message {
                    Message::Catalog {
                        entries,
                        active_id: _,
                    } => {
                        model.set_catalog(entries);
                        frame_changed = true;
                    }
                    Message::OpenEditor {
                        example_id,
                        label,
                        application,
                        revision,
                        units,
                        migration,
                        migration_stage,
                    } => {
                        model.sync_editor(&state);
                        model.active = StoredProject {
                            id: example_id,
                            label,
                            origin: ProjectOrigin::BuiltIn,
                            application,
                            units,
                        };
                        model.active_file = model.active.units.len().saturating_sub(1);
                        model.revision = revision;
                        model.pending_inspection = None;
                        model.runtime_sequence = None;
                        model.runtime_value_path = None;
                        model.language = None;
                        model.source_publication_revision = None;
                        model.install_migration(migration, migration_stage);
                        model.clear_persistence_cache();
                        model.runtime_value = "Compiling preview...".to_owned();
                        state.replace_source(model.source());
                        language_worker.submit(
                            model.revision,
                            model.active_file,
                            model.active.units.clone(),
                        );
                        frame_changed = true;
                    }
                    Message::PreviewStats(stats) => {
                        model.perf = perf_line(&stats);
                        frame_changed = true;
                    }
                    Message::PreviewStatus {
                        revision,
                        ok,
                        message,
                    } => {
                        model.runtime_value = if ok {
                            "Preview current".to_owned()
                        } else {
                            "Unavailable while source has errors".to_owned()
                        };
                        state.set_status(format!(
                            "r{revision} {}: {message}",
                            if ok { "ready" } else { "error" }
                        ));
                        if ok && revision == model.revision {
                            request_inspection(&mut model, &state, &mut writer)?;
                        }
                        frame_changed = true;
                    }
                    Message::PreviewRuntimeChanged {
                        revision,
                        runtime_sequence,
                    } => {
                        let inspection_active = model.pending_inspection.is_some()
                            || model.runtime_value_path.is_some();
                        if note_runtime_change(&mut model, revision, runtime_sequence)
                            && inspection_active
                        {
                            request_inspection(&mut model, &state, &mut writer)?;
                            frame_changed = true;
                        }
                    }
                    Message::PreviewInspectResult {
                        request_id,
                        revision,
                        runtime_sequence,
                        path,
                        ok,
                        value,
                        authority,
                    } => {
                        match apply_inspection_result(
                            &mut model,
                            request_id,
                            revision,
                            runtime_sequence,
                            path,
                            ok,
                            value,
                            authority,
                        ) {
                            InspectionResultDisposition::Ignored => {}
                            InspectionResultDisposition::Applied => frame_changed = true,
                            InspectionResultDisposition::Retry => {
                                request_inspection(&mut model, &state, &mut writer)?;
                                frame_changed = true;
                            }
                        }
                    }
                    Message::PreviewTestResult {
                        request_id,
                        passed,
                        message,
                    } => {
                        state.set_status(format!(
                            "TEST #{request_id} {}: {message}",
                            if passed { "passed" } else { "failed" }
                        ));
                        frame_changed = true;
                    }
                    Message::PreviewMigrationStatus(status) => {
                        if apply_migration_status(&mut model, status) {
                            state.set_status(model.migration_status_text());
                            frame_changed = true;
                        }
                    }
                    Message::PreviewPersistenceSnapshot(snapshot) => {
                        let operation = snapshot.last_operation.clone();
                        if model.apply_persistence_snapshot(*snapshot) {
                            if let Some(operation) = operation {
                                state.set_status(operation.message);
                            }
                            frame_changed = true;
                        }
                    }
                    Message::PreviewPersistenceArtifact {
                        request_id: _,
                        revision,
                        artifact,
                    } => {
                        if revision == model.revision {
                            state.set_status(format!(
                                "Cached {} bytes of canonical CBOR for Import Preview",
                                artifact.bytes.len()
                            ));
                            model.state_artifact = Some(artifact);
                            frame_changed = true;
                        }
                    }
                    Message::Ready {
                        role: Role::Preview,
                    } => {
                        state.set_status("Preview connected");
                        frame_changed = true;
                    }
                    Message::Shutdown => return Ok(()),
                    other => {
                        return Err(format!("invalid desktop-to-dev message: {other:?}").into());
                    }
                }
            }
            Wake::Language(Some(snapshot)) => {
                if snapshot.revision == model.revision && snapshot.file_index == model.active_file {
                    let snapshot_revision = snapshot.revision;
                    model.language = Some(snapshot);
                    model.sync_editor(&state);
                    if model.active.origin == ProjectOrigin::Custom {
                        persistence
                            .submit(PersistRequest::Save(model.active.clone()))
                            .map_err(|error| format!("custom autosave failed: {error}"))?;
                    }
                    if model.take_source_publication(snapshot_revision) {
                        writer.send(&Message::DevSourceChanged {
                            application: model.active.application.clone(),
                            revision: model.revision,
                            units: model.active.units.clone(),
                        })?;
                    }
                    frame_changed = true;
                }
            }
            Wake::Language(None) => return Err("language worker stopped".into()),
        }

        while let Some(result) = persistence.try_result() {
            state.set_status(match result.result {
                Ok(()) => format!("Saved {}", result.project_id),
                Err(error) => format!("Save failed: {error}"),
            });
            frame_changed = true;
        }

        let mut render_changed = frame_changed;
        if frame_changed {
            let next = build_frame(&model, &state);
            let patches = diff_document_frames(view.frame(), &next);
            if !patches.is_empty() {
                let update = view.apply_patches(patches, &mut columns)?;
                render_changed |= update.layout_changed || update.render_changed;
            }
        }
        if interaction_changed || frame_changed {
            let update =
                view.set_interaction_state(state.hovered(), state.focused_target(), &mut columns)?;
            render_changed |= update.layout_changed || update.render_changed;
        }
        if render_changed {
            transaction.mark_dirty();
            emit_dev_targets(&observer, &view, &mut observed_targets);
        }
        if let Some(presented) = transaction.present(&mut product, &mut host, &view).await? {
            emit_presented(&observer, &presented);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_action(
    action: DevAction,
    model: &mut DevModel,
    state: &mut DevState,
    store: &ProjectStore,
    persistence: &PersistenceWorker,
    language: &LanguageWorker,
    writer: &mut Connection,
    clipboard: &mut Option<arboard::Clipboard>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !matches!(&action, DevAction::MigrationStartOver) {
        model.start_over_armed = false;
    }
    if !matches!(&action, DevAction::PersistenceClearAll) {
        model.clear_all_armed = false;
    }
    if !matches!(&action, DevAction::PersistenceClearSelected) {
        model.clear_selected_armed = false;
    }
    match action {
        DevAction::None => {}
        DevAction::Previous | DevAction::Next => {
            if let Some(id) =
                adjacent_id(&model.catalog, &model.active.id, action == DevAction::Next)
            {
                select_project(id, model, state, persistence, language, writer)?;
            }
        }
        DevAction::SelectExample(id) => {
            select_project(id, model, state, persistence, language, writer)?;
        }
        DevAction::SelectFile(index) if index < model.active.units.len() => {
            model.sync_editor(state);
            model.active_file = index;
            model.language = None;
            model.source_publication_revision = None;
            state.replace_source(model.source());
            language.submit(model.revision, index, model.active.units.clone());
        }
        DevAction::SelectFile(_) => {}
        DevAction::SelectMigrationStage(stage_id) => {
            if model.migration.as_ref().is_some_and(|migration| {
                model
                    .active_migration_stage
                    .as_deref()
                    .is_some_and(|active| is_forward_migration_stage(migration, active, &stage_id))
            }) {
                model.selected_migration_stage = Some(stage_id.clone());
                state.set_status(format!("Selected migration target {stage_id}"));
            }
        }
        DevAction::NewProject => {
            model.sync_editor(state);
            let project = store.create_custom(model.catalog.iter().map(|entry| entry.id.clone()));
            persistence.submit(PersistRequest::Save(project.clone()))?;
            model.custom.insert(project.id.clone(), project.clone());
            model.upsert_custom_catalog(&project);
            model.active = project;
            model.install_migration(None, None);
            model.active_file = 0;
            model.revision = model.revision.saturating_add(1);
            model.language = None;
            model.source_publication_revision = None;
            state.replace_source(model.source());
            state.begin_rename(&model.active.label);
            language.submit(model.revision, 0, model.active.units.clone());
            writer.send(&Message::DevRun {
                application: model.active.application.clone(),
                revision: model.revision,
                units: model.active.units.clone(),
            })?;
            state.set_status("Created local custom example; enter its name");
        }
        DevAction::NewFile if model.active.origin == ProjectOrigin::Custom => {
            model.sync_editor(state);
            let name = next_custom_file_name(&model.active);
            state.begin_new_file(&name);
            state.set_status("Enter a name for the new source file");
        }
        DevAction::NewFile => {}
        DevAction::BeginRename if model.active.origin == ProjectOrigin::Custom => {
            state.begin_rename(&model.active.label);
            state.set_status("Rename custom example");
        }
        DevAction::BeginRename => {}
        DevAction::BeginFileRename if model.active.origin == ProjectOrigin::Custom => {
            let name = model
                .active
                .units
                .get(model.active_file)
                .and_then(|unit| Path::new(&unit.path).file_name())
                .and_then(|name| name.to_str())
                .unwrap_or("RUN.bn")
                .to_owned();
            state.begin_file_rename(model.active_file, &name);
            state.set_status("Rename source file");
        }
        DevAction::BeginFileRename => {}
        DevAction::CommitRename if model.active.origin == ProjectOrigin::Custom => {
            let requested = state.rename_text().unwrap_or_default();
            match state.name_edit_target() {
                Some(NameEditTarget::Project) => match normalized_custom_label(&requested) {
                    Ok(label)
                        if !model.catalog.iter().any(|entry| {
                            entry.id != model.active.id && entry.label.eq_ignore_ascii_case(&label)
                        }) =>
                    {
                        model.sync_editor(state);
                        model.active.label.clone_from(&label);
                        model
                            .custom
                            .insert(model.active.id.clone(), model.active.clone());
                        model.upsert_custom_catalog(&model.active.clone());
                        persistence.submit(PersistRequest::Save(model.active.clone()))?;
                        state.finish_rename();
                        state.set_status(format!("Renamed custom example to {label}"));
                    }
                    Ok(_) => state.set_status("An example with that name already exists"),
                    Err(error) => state.set_status(error),
                },
                Some(NameEditTarget::NewFile) => {
                    model.sync_editor(state);
                    match model.active.add_file(&requested) {
                        Ok(index) => {
                            model.active_file = index;
                            commit_custom_file_change(
                                model,
                                state,
                                persistence,
                                language,
                                writer,
                                "Created source file",
                            )?;
                        }
                        Err(error) => state.set_status(format!("Cannot create file: {error}")),
                    }
                }
                Some(NameEditTarget::File(index)) => {
                    model.sync_editor(state);
                    match model.active.rename_file(index, &requested) {
                        Ok(()) => {
                            model.active_file = index;
                            commit_custom_file_change(
                                model,
                                state,
                                persistence,
                                language,
                                writer,
                                "Renamed source file",
                            )?;
                        }
                        Err(error) => state.set_status(format!("Cannot rename file: {error}")),
                    }
                }
                None => state.finish_rename(),
            }
        }
        DevAction::CommitRename => state.finish_rename(),
        DevAction::CancelRename => {
            state.finish_rename();
            state.set_status("Rename cancelled");
        }
        DevAction::RemoveProject if model.active.origin == ProjectOrigin::Custom => {
            let removed = model.active.id.clone();
            persistence.submit(PersistRequest::Remove(removed.clone()))?;
            model.custom.remove(&removed);
            model.catalog.retain(|entry| entry.id != removed);
            if let Some(next) = model.catalog.iter().find(|entry| !entry.custom) {
                writer.send(&Message::DevSelectExample {
                    example_id: next.id.clone(),
                })?;
            }
            state.set_status("Removed local custom example");
        }
        DevAction::RemoveProject => {}
        DevAction::RemoveFile if model.active.origin == ProjectOrigin::Custom => {
            model.sync_editor(state);
            match model.active.remove_file(model.active_file) {
                Ok(()) => {
                    model.active_file = model
                        .active_file
                        .min(model.active.units.len().saturating_sub(1));
                    commit_custom_file_change(
                        model,
                        state,
                        persistence,
                        language,
                        writer,
                        "Removed source file",
                    )?;
                }
                Err(error) => state.set_status(format!("Cannot remove file: {error}")),
            }
        }
        DevAction::RemoveFile => {}
        DevAction::Run => {
            model.sync_editor(state);
            model.revision = model.revision.saturating_add(1);
            model.source_publication_revision = None;
            writer.send(&Message::DevRun {
                application: model.active.application.clone(),
                revision: model.revision,
                units: model.active.units.clone(),
            })?;
            state.set_status("Compiling...");
        }
        DevAction::Reset => {
            if model.active.origin == ProjectOrigin::BuiltIn {
                writer.send(&Message::DevReset)?;
                state.set_status("Resetting versioned example...");
            } else {
                let project = store
                    .load_custom()?
                    .into_iter()
                    .find(|project| project.id == model.active.id)
                    .unwrap_or_else(|| model.active.clone());
                model.active = project;
                model.active_file = model.active_file.min(model.active.units.len() - 1);
                state.replace_source(model.source());
                state.set_status("Reloaded local example");
            }
        }
        DevAction::Test => {
            model.sync_editor(state);
            if model.migration.is_none() {
                model.revision = model.revision.saturating_add(1);
            }
            model.source_publication_revision = None;
            model.request_id = model.request_id.saturating_add(1);
            writer.send(&Message::DevTest {
                request_id: model.request_id,
                application: model.active.application.clone(),
                revision: model.revision,
                units: model.active.units.clone(),
            })?;
            state.set_status("TEST running in preview...");
        }
        DevAction::MigrationPreview => {
            let Some(stage_id) = model.selected_migration_stage.clone() else {
                state.set_status("Select a forward migration stage");
                return Ok(());
            };
            send_migration_command(
                model,
                writer,
                MigrationCommand::Preview {
                    stage_id: stage_id.clone(),
                },
            )?;
            state.set_status(format!("Previewing migration to {stage_id}..."));
        }
        DevAction::MigrationActivate => {
            let Some(stage_id) = model.selected_migration_stage.clone() else {
                state.set_status("Select a forward migration stage");
                return Ok(());
            };
            if model
                .migration_status
                .as_ref()
                .and_then(|status| status.previewed_stage.as_deref())
                != Some(stage_id.as_str())
            {
                state.set_status("Preview this exact stage before activation");
                return Ok(());
            }
            send_migration_command(
                model,
                writer,
                MigrationCommand::Activate {
                    stage_id: stage_id.clone(),
                },
            )?;
            state.set_status(format!("Activating migration to {stage_id}..."));
        }
        DevAction::MigrationRestart => {
            send_migration_command(model, writer, MigrationCommand::Restart)?;
            state.set_status("Restarting from durable state...");
        }
        DevAction::MigrationStartOver => {
            if model.start_over_armed {
                model.start_over_armed = false;
                send_migration_command(
                    model,
                    writer,
                    MigrationCommand::StartOver { confirmed: true },
                )?;
                state.set_status("Starting over from current stage defaults...");
            } else {
                model.start_over_armed = true;
                state.set_status("Start Over deletes this namespace's authority; confirm again");
            }
        }
        DevAction::SelectInspector(mode) => {
            model.inspector_mode = mode;
            if mode == InspectorMode::Outbox {
                model.outbox_offset = model.outbox_offset.min(
                    model
                        .persistence_snapshot
                        .as_ref()
                        .map_or(0, |snapshot| snapshot.outbox.samples.len())
                        .saturating_sub(OUTBOX_WINDOW_ROWS),
                );
            }
        }
        DevAction::PersistenceFlush => {
            send_persistence_command(model, writer, PersistenceCommand::Flush)?;
            state.set_status("Flushing pending durable state...");
        }
        DevAction::PersistenceCompact => {
            send_persistence_command(model, writer, PersistenceCommand::Compact)?;
            state.set_status("Running persistence maintenance...");
        }
        DevAction::PersistenceClearAll => {
            if model.clear_all_armed {
                model.clear_all_armed = false;
                send_persistence_command(
                    model,
                    writer,
                    PersistenceCommand::ClearAll { confirmed: true },
                )?;
                state.set_status("Clearing all application authority and outbox state...");
            } else {
                model.clear_all_armed = true;
                state.set_status(
                    "Clear All deletes this application's authority and outbox; confirm again",
                );
            }
        }
        DevAction::PersistenceClearSelected => {
            let Some(selection) = model.selected_authority.clone() else {
                state.set_status("Select an authoritative memory or list first");
                return Ok(());
            };
            if model.clear_selected_armed {
                model.clear_selected_armed = false;
                send_persistence_command(
                    model,
                    writer,
                    PersistenceCommand::ClearSelected {
                        selection,
                        confirmed: true,
                    },
                )?;
                state.set_status("Clearing the selected stored authority...");
            } else {
                model.clear_selected_armed = true;
                state.set_status(format!(
                    "Clear `{}` stored authority; confirm again",
                    selection.semantic_path
                ));
            }
        }
        DevAction::PersistenceExport => {
            send_persistence_command(model, writer, PersistenceCommand::ExportState)?;
            state.set_status("Exporting bounded canonical CBOR state...");
        }
        DevAction::PersistenceImportPreview => {
            let Some(artifact) = model.state_artifact.clone() else {
                state.set_status("Export or load a bounded state artifact before Import Preview");
                return Ok(());
            };
            send_persistence_command(
                model,
                writer,
                PersistenceCommand::ImportPreview { artifact },
            )?;
            state.set_status("Settling imported state in an isolated preview...");
        }
        DevAction::PersistenceActivateImport => {
            let Some(preview_id) = model
                .persistence_snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.import_preview.as_ref())
                .map(|preview| preview.preview_id)
            else {
                state.set_status("Run Import Preview before activation");
                return Ok(());
            };
            send_persistence_command(
                model,
                writer,
                PersistenceCommand::ActivateImport { preview_id },
            )?;
            state.set_status(format!(
                "Activating Import Preview #{preview_id} durably..."
            ));
        }
        DevAction::OutboxPrevious => {
            model.outbox_offset = model.outbox_offset.saturating_sub(OUTBOX_WINDOW_ROWS);
        }
        DevAction::OutboxNext => {
            let max_offset = model
                .persistence_snapshot
                .as_ref()
                .map_or(0, |snapshot| snapshot.outbox.samples.len())
                .saturating_sub(OUTBOX_WINDOW_ROWS);
            model.outbox_offset = model
                .outbox_offset
                .saturating_add(OUTBOX_WINDOW_ROWS)
                .min(max_offset);
        }
        DevAction::Save => {
            model.sync_editor(state);
            persistence.submit(PersistRequest::Save(model.active.clone()))?;
            state.set_status(match model.active.origin {
                ProjectOrigin::BuiltIn => "Saving versioned source...",
                ProjectOrigin::Custom => "Saving local custom source...",
            });
        }
        DevAction::Format => {
            let path = model
                .active
                .units
                .get(model.active_file)
                .map(|unit| unit.path.as_str())
                .unwrap_or("RUN.bn");
            match boon_parser::format_source_unit(path, state.source()) {
                Ok(source) => {
                    state.format(source);
                    model.sync_editor(state);
                    model.revision = model.revision.saturating_add(1);
                    model.source_publication_revision = Some(model.revision);
                    language.submit(
                        model.revision,
                        model.active_file,
                        model.active.units.clone(),
                    );
                }
                Err(error) => state.set_status(format!("Format failed: {error}")),
            }
        }
        DevAction::Clipboard(action) => {
            let clipboard = clipboard_instance(clipboard)?;
            let mut edited = false;
            match action {
                ClipboardAction::Copy => {
                    let selected = state.selected_text();
                    if !selected.is_empty() {
                        clipboard.set_text(selected)?;
                    }
                }
                ClipboardAction::Cut => {
                    let selected = state.selected_text();
                    if !selected.is_empty() {
                        clipboard.set_text(selected)?;
                        edited = state.cut_selection();
                    }
                }
                ClipboardAction::Paste => {
                    let text = clipboard.get_text()?;
                    edited = state.paste(&text);
                }
            }
            if edited {
                model.sync_editor(state);
                model.revision = model.revision.saturating_add(1);
                model.source_publication_revision = Some(model.revision);
                language.submit(
                    model.revision,
                    model.active_file,
                    model.active.units.clone(),
                );
            }
        }
        DevAction::Close => {
            let _ = writer.send(&Message::Shutdown);
        }
    }
    Ok(())
}

fn select_project(
    id: String,
    model: &mut DevModel,
    state: &mut DevState,
    persistence: &PersistenceWorker,
    language: &LanguageWorker,
    writer: &mut Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    model.sync_editor(state);
    if model.active.origin == ProjectOrigin::Custom {
        persistence.submit(PersistRequest::Save(model.active.clone()))?;
    }
    if let Some(project) = model.custom.get(&id).cloned() {
        model.active = project;
        model.install_migration(None, None);
        model.active_file = model.active.units.len().saturating_sub(1);
        model.revision = model.revision.saturating_add(1);
        model.language = None;
        model.source_publication_revision = None;
        state.replace_source(model.source());
        language.submit(
            model.revision,
            model.active_file,
            model.active.units.clone(),
        );
        writer.send(&Message::DevRun {
            application: model.active.application.clone(),
            revision: model.revision,
            units: model.active.units.clone(),
        })?;
        state.set_status("Opened local custom example");
    } else {
        writer.send(&Message::DevSelectExample { example_id: id })?;
        state.set_status("Opening versioned example...");
    }
    Ok(())
}

fn send_migration_command(
    model: &mut DevModel,
    writer: &mut Connection,
    command: MigrationCommand,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    model.request_id = model.request_id.saturating_add(1);
    writer.send(&Message::DevMigrationCommand {
        request_id: model.request_id,
        revision: model.revision,
        command,
    })?;
    Ok(())
}

fn send_persistence_command(
    model: &mut DevModel,
    writer: &mut Connection,
    command: PersistenceCommand,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    model.request_id = model.request_id.saturating_add(1);
    writer.send(&Message::DevPersistenceCommand {
        request_id: model.request_id,
        revision: model.revision,
        command,
    })?;
    Ok(())
}

fn apply_migration_status(model: &mut DevModel, status: MigrationStatus) -> bool {
    if status.revision != model.revision {
        return false;
    }
    let Some(migration) = model.migration.as_ref() else {
        return false;
    };
    if migration.stage(&status.active_stage).is_none() {
        return false;
    }
    model.active_migration_stage = Some(status.active_stage.clone());
    match status.operation {
        MigrationOperation::Activated => {
            model.selected_migration_stage = next_migration_stage(migration, &status.active_stage);
        }
        MigrationOperation::Previewed => {
            if let Some(target) = status.target_stage.as_ref()
                && is_forward_migration_stage(migration, &status.active_stage, target)
            {
                model.selected_migration_stage = Some(target.clone());
            }
        }
        MigrationOperation::Opened => {
            let selected_is_forward =
                model
                    .selected_migration_stage
                    .as_deref()
                    .is_some_and(|selected| {
                        is_forward_migration_stage(migration, &status.active_stage, selected)
                    });
            if !selected_is_forward {
                model.selected_migration_stage =
                    next_migration_stage(migration, &status.active_stage);
            }
        }
        MigrationOperation::Restarted
        | MigrationOperation::StartedOver
        | MigrationOperation::Failed => {}
    }
    model.start_over_armed = false;
    model.migration_status = Some(status);
    true
}

fn next_migration_stage(migration: &MigrationBundle, active_stage: &str) -> Option<String> {
    migration
        .stages
        .iter()
        .position(|stage| stage.id == active_stage)
        .and_then(|index| migration.stages.get(index + 1))
        .map(|stage| stage.id.clone())
}

fn is_forward_migration_stage(
    migration: &MigrationBundle,
    active_stage: &str,
    target_stage: &str,
) -> bool {
    let active = migration
        .stages
        .iter()
        .position(|stage| stage.id == active_stage);
    let target = migration
        .stages
        .iter()
        .position(|stage| stage.id == target_stage);
    matches!((active, target), (Some(active), Some(target)) if target > active)
}

fn clipboard_instance(
    clipboard: &mut Option<arboard::Clipboard>,
) -> Result<&mut arboard::Clipboard, arboard::Error> {
    if clipboard.is_none() {
        *clipboard = Some(arboard::Clipboard::new()?);
    }
    Ok(clipboard.as_mut().expect("clipboard initialized"))
}

fn build_frame(model: &DevModel, state: &DevState) -> boon_document::DocumentFrame {
    let inspector = inspector(model, state);
    let paths = model.source_paths();
    let migration_status = model.migration_status_text();
    let migration = model
        .migration
        .as_ref()
        .zip(model.active_migration_stage.as_deref())
        .map(|(migration, active_stage)| MigrationUiState {
            stages: &migration.stages,
            active_stage,
            selected_stage: model
                .selected_migration_stage
                .as_deref()
                .unwrap_or(active_stage),
            previewed_stage: model
                .migration_status
                .as_ref()
                .and_then(|status| status.previewed_stage.as_deref()),
            status: &migration_status,
            start_over_armed: model.start_over_armed,
        });
    dev_frame(DevFrameState {
        catalog: &model.catalog,
        active_id: &model.active.id,
        example_label: &model.active.label,
        origin: model.active.origin,
        source_paths: &paths,
        active_file: model.active_file,
        buffer: state.buffer(),
        rename_buffer: state.rename_buffer(),
        rename_prompt: state.rename_prompt(),
        editor_scroll: state.editor_scroll(),
        language: model.language.as_ref(),
        migration,
        inspector: InspectorState {
            symbol: &inspector.symbol,
            static_type: &inspector.static_type,
            detail: &inspector.detail,
            current_value: &inspector.current_value,
        },
        persistence: PersistenceUiState {
            snapshot: model.persistence_snapshot.as_ref(),
            mode: model.inspector_mode,
            outbox_offset: model.outbox_offset,
            clear_all_armed: model.clear_all_armed,
            clear_selected_armed: model.clear_selected_armed,
            selected_authority: model.selected_authority.as_ref(),
            has_state_artifact: model.state_artifact.is_some(),
        },
        status: state.status(),
        perf: &model.perf,
    })
}

fn inspector(model: &DevModel, state: &DevState) -> InspectorOwned {
    let byte = state
        .buffer()
        .byte_for_position(state.inspection_position());
    let source = state.source();
    let symbol = symbol_at(&source, byte);
    let hint = model
        .language
        .as_ref()
        .filter(|snapshot| snapshot.revision == model.revision)
        .and_then(|snapshot| snapshot.hint_at(byte));
    InspectorOwned {
        symbol,
        static_type: hint.map_or_else(
            || "No type at caret".to_owned(),
            |hint| hint.compact_label.clone(),
        ),
        detail: hint.map_or_else(String::new, |hint| {
            format!("{}\n{}", hint.category, hint.detail_label)
        }),
        current_value: model.runtime_value.clone(),
    }
}

fn symbol_at(source: &str, byte: usize) -> String {
    let is_symbol =
        |value: u8| value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-' | b'/' | b'.');
    let bytes = source.as_bytes();
    let mut start = byte.min(bytes.len());
    let mut end = start;
    while start > 0 && is_symbol(bytes[start - 1]) {
        start -= 1;
    }
    while end < bytes.len() && is_symbol(bytes[end]) {
        end += 1;
    }
    source.get(start..end).unwrap_or_default().to_owned()
}

fn request_inspection(
    model: &mut DevModel,
    state: &DevState,
    writer: &mut Connection,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(message) = prepare_inspection_request(model, state) {
        writer.send(&message)?;
    }
    Ok(())
}

fn prepare_inspection_request(model: &mut DevModel, state: &DevState) -> Option<Message> {
    let byte = state
        .buffer()
        .byte_for_position(state.inspection_position());
    let source = state.source();
    let path = symbol_at(&source, byte);
    if let Some(literal) = literal_value(&path) {
        model.pending_inspection = None;
        model.runtime_value_path = None;
        model.selected_authority = None;
        model.clear_selected_armed = false;
        model.runtime_value = literal;
        return None;
    }
    if !is_runtime_path(&path) {
        model.pending_inspection = None;
        model.runtime_value_path = None;
        model.selected_authority = None;
        model.clear_selected_armed = false;
        model.runtime_value = "No runtime binding at this position".to_owned();
        return None;
    }
    let desired_sequence = desired_runtime_sequence(model);
    if model
        .pending_inspection
        .as_ref()
        .is_some_and(|(_, revision, pending)| *revision == model.revision && pending == &path)
        || model
            .runtime_value_path
            .as_ref()
            .is_some_and(|(revision, runtime_sequence, current)| {
                *revision == model.revision
                    && *runtime_sequence >= desired_sequence
                    && current == &path
            })
    {
        return None;
    }
    let preserve_current_value = model
        .runtime_value_path
        .as_ref()
        .is_some_and(|(revision, _, current)| *revision == model.revision && current == &path);
    model.inspect_request_id = model.inspect_request_id.saturating_add(1);
    let request_id = model.inspect_request_id;
    model.pending_inspection = Some((request_id, model.revision, path.clone()));
    if !preserve_current_value {
        model.runtime_value = "Reading current value...".to_owned();
        model.selected_authority = None;
        model.clear_selected_armed = false;
    }
    Some(Message::DevInspect {
        request_id,
        revision: model.revision,
        path,
    })
}

fn desired_runtime_sequence(model: &DevModel) -> u64 {
    model
        .runtime_sequence
        .filter(|(revision, _)| *revision == model.revision)
        .map_or(0, |(_, runtime_sequence)| runtime_sequence)
}

fn note_runtime_change(model: &mut DevModel, revision: u64, runtime_sequence: u64) -> bool {
    if revision != model.revision
        || model
            .runtime_sequence
            .is_some_and(|(current_revision, current_sequence)| {
                current_revision == revision && current_sequence >= runtime_sequence
            })
    {
        return false;
    }
    model.runtime_sequence = Some((revision, runtime_sequence));
    true
}

#[allow(clippy::too_many_arguments)]
fn apply_inspection_result(
    model: &mut DevModel,
    request_id: u64,
    revision: u64,
    runtime_sequence: u64,
    path: String,
    ok: bool,
    value: String,
    authority: Option<AuthoritySelection>,
) -> InspectionResultDisposition {
    if model.pending_inspection.as_ref() != Some(&(request_id, revision, path.clone())) {
        return InspectionResultDisposition::Ignored;
    }
    model.pending_inspection = None;
    if revision != model.revision {
        return InspectionResultDisposition::Ignored;
    }
    if runtime_sequence < desired_runtime_sequence(model) {
        return InspectionResultDisposition::Retry;
    }
    model.runtime_value = value;
    model.selected_authority = ok.then_some(authority).flatten();
    model.clear_selected_armed = false;
    model.runtime_value_path = ok.then_some((revision, runtime_sequence, path));
    InspectionResultDisposition::Applied
}

fn is_runtime_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('.').all(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                && chars.all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
                })
        })
}

fn literal_value(value: &str) -> Option<String> {
    if value.parse::<i64>().is_ok() || matches!(value, "True" | "False" | "Null") {
        Some(value.to_owned())
    } else {
        None
    }
}

fn editor_column(view: &RetainedView, state: &DevState, line: usize, pointer_x: f32) -> usize {
    let slot = line.saturating_sub(editor_first_line(state.editor_scroll()));
    let id = format!("dev.editor.code.{slot}");
    let Some(bounds) = view.node_bounds(&id) else {
        return 0;
    };
    let text = state.buffer().line(line);
    let edges = boon_native_gpu::editor_text_column_edges(&text, 14.0, 23.0, "JetBrains Mono", "");
    let local = (pointer_x - bounds.x - 5.0).max(0.0);
    edges
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| (**left - local).abs().total_cmp(&(**right - local).abs()))
        .map_or(0, |(index, _)| index)
}

fn editor_slot_at(view: &RetainedView, target: Option<&str>, pointer_y: f32) -> Option<usize> {
    if let Some(slot) = target.and_then(editor_line_from_target) {
        return Some(slot);
    }
    if !target.is_some_and(|target| target == DEV_EDITOR || target.starts_with("dev.editor.")) {
        return None;
    }
    let bounds = view.node_bounds("dev.editor.lines")?;
    let slot = ((pointer_y - bounds.y).max(0.0) / 23.0).floor() as usize;
    (slot < 36).then_some(slot)
}

fn emit_dev_targets(
    observer: &Option<ObserverClient>,
    view: &RetainedView,
    observed: &mut BTreeMap<String, (f32, f32)>,
) {
    for node in [
        DEV_PREVIOUS,
        DEV_NEXT,
        DEV_RUN,
        DEV_RESET,
        DEV_TEST,
        DEV_SAVE,
        DEV_FORMAT,
        DEV_NEW,
        DEV_REMOVE,
        DEV_RENAME,
        DEV_FILE_NEW,
        DEV_FILE_RENAME,
        DEV_FILE_REMOVE,
        DEV_RENAME_INPUT,
        DEV_RENAME_SAVE,
        DEV_RENAME_CANCEL,
        DEV_EDITOR,
        DEV_EDITOR_INPUT_TARGET,
    ] {
        let Some(target) = view.target_for_source(node, None) else {
            continue;
        };
        let center = (target.center_x, target.center_y);
        if observed.get(node) == Some(&center) {
            continue;
        }
        observed.insert(node.to_owned(), center);
        emit(
            observer,
            ObserverEvent::RoleTarget {
                role: ObserverRole::Dev,
                node: node.to_owned(),
                x: center.0,
                y: center.1,
            },
        );
    }
}

fn native_target_name(view: &RetainedView, event: &HostEvent, state: &DevState) -> Option<String> {
    match event {
        HostEvent::Pointer(pointer) => view.hit(pointer.x, pointer.y).map(str::to_owned),
        HostEvent::Wheel(wheel) => view.hit(wheel.x, wheel.y).map(str::to_owned),
        HostEvent::Keyboard(_) | HostEvent::TextInput(_) | HostEvent::Ime(_) => {
            state.focused_target().map(str::to_owned)
        }
        _ => None,
    }
}

fn observe_input(
    observer: &Option<ObserverClient>,
    envelope: &HostEventEnvelope,
    target: Option<String>,
    visible_change: bool,
) {
    let (pointer_x, pointer_y) = event_position(&envelope.event);
    emit(
        observer,
        ObserverEvent::InputAccepted(InputAccepted {
            role: ObserverRole::Dev,
            event_sequence: envelope.sequence,
            real_os: envelope.origin == HostEventOrigin::RealOs,
            callback_to_host_ns: envelope.callback_to_host_ns.get(),
            surface_epoch: envelope.surface_epoch,
            kind: input_kind(&envelope.event),
            pointer_button_pressed: pointer_button_pressed(&envelope.event),
            pointer_x,
            pointer_y,
            target,
            target_source_path: None,
            event_digest: host_event_digest(envelope),
            visible_change,
        }),
    );
}

fn event_position(event: &HostEvent) -> (Option<f32>, Option<f32>) {
    match event {
        HostEvent::Pointer(pointer) => (Some(pointer.x), Some(pointer.y)),
        HostEvent::Wheel(wheel) => (Some(wheel.x), Some(wheel.y)),
        _ => (None, None),
    }
}

fn emit_presented(observer: &Option<ObserverClient>, frame: &PresentedFrame) {
    let drops = observer
        .as_ref()
        .map(ObserverClient::dropped_count)
        .unwrap_or(0);
    emit(observer, frame.observer_event(ObserverRole::Dev, drops));
}

fn emit(observer: &Option<ObserverClient>, event: ObserverEvent) {
    if let Some(observer) = observer {
        observer.emit(event);
    }
}

fn viewport(host: &NativeSurfaceHost) -> Viewport {
    let native = host.viewport();
    Viewport {
        surface: host.epoch(),
        width: native.logical_size.width,
        height: native.logical_size.height,
        scale: native.scale,
    }
}

fn adjacent_id(entries: &[CatalogItem], active: &str, next: bool) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let index = entries.iter().position(|entry| entry.id == active)?;
    let index = if next {
        (index + 1) % entries.len()
    } else if index == 0 {
        entries.len() - 1
    } else {
        index - 1
    };
    entries.get(index).map(|entry| entry.id.clone())
}

fn next_custom_file_name(project: &StoredProject) -> String {
    for ordinal in 1_u64.. {
        let candidate = if ordinal == 1 {
            "Module.bn".to_owned()
        } else {
            format!("Module{ordinal}.bn")
        };
        if !project.units.iter().any(|unit| {
            Path::new(&unit.path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(&candidate))
        }) {
            return candidate;
        }
    }
    unreachable!("custom source file name space exhausted")
}

fn commit_custom_file_change(
    model: &mut DevModel,
    state: &mut DevState,
    persistence: &PersistenceWorker,
    language: &LanguageWorker,
    writer: &mut Connection,
    status: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    model
        .custom
        .insert(model.active.id.clone(), model.active.clone());
    persistence.submit(PersistRequest::Save(model.active.clone()))?;
    model.revision = model.revision.saturating_add(1);
    model.language = None;
    model.source_publication_revision = None;
    state.replace_source(model.source());
    language.submit(
        model.revision,
        model.active_file,
        model.active.units.clone(),
    );
    writer.send(&Message::DevRun {
        application: model.active.application.clone(),
        revision: model.revision,
        units: model.active.units.clone(),
    })?;
    state.set_status(status);
    Ok(())
}

fn normalized_custom_label(value: &str) -> Result<String, String> {
    let label = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(64)
        .collect::<String>();
    if label.is_empty() {
        Err("Custom example name cannot be empty".to_owned())
    } else {
        Ok(label)
    }
}

fn perf_line(stats: &PreviewStats) -> String {
    let mode = match stats.frame_mode {
        FrameMode::Idle => "idle",
        FrameMode::Burst => "burst",
        FrameMode::Probe => "probe",
    };
    let proof = match stats.proof_mode {
        ProofMode::Off => "off",
        ProofMode::Trace => "trace",
        ProofMode::Readback => "readback",
    };
    let persistence = if !stats.persistence_worker_alive {
        "state unavailable".to_owned()
    } else if !stats.persistence_error.is_empty() {
        "state error".to_owned()
    } else {
        format!(
            "state v{} e{}/t{} pending {} q{}{}",
            stats.persistence_schema_version,
            stats.persistence_durable_epoch,
            stats.persistence_durable_turn,
            stats.persistence_pending_turns,
            stats.persistence_queue_depth,
            if stats.persistence_accepting {
                ""
            } else {
                " paused"
            },
        )
    };
    format!(
        "Preview {mode}, last {:.2}ms, render {:.2}ms, {persistence}, proof {proof}, misses {}, drops {}, age {}ms",
        f64::from(stats.input_to_present_micros) / 1000.0,
        f64::from(stats.render_micros) / 1000.0,
        stats.missed_frames,
        stats.dropped_snapshots,
        stats.sample_age_millis,
    )
}
