use std::collections::BTreeMap;
use std::path::Path;
use std::thread;

use boon_document::{DocumentNodeId, DocumentPatch, TextValue};
use boon_document_model::ScrollState;
use boon_host::{HostEvent, HostEventEnvelope, HostEventOrigin, Viewport};
use boon_native_app_window::{NativeRoleResult, NativeSurfaceHost};
use futures::channel::mpsc;
use futures::{FutureExt, StreamExt, pin_mut, select};

use crate::dev_state::{DevAction, DevChange, DevState};
use crate::frame::{
    NativeFrameTransaction, PresentedFrame, ProductFrame, drain_native_events, input_kind,
    pointer_button_pressed,
};
use crate::observer::{InputAccepted, ObserverClient, ObserverEvent, ObserverRole};
use crate::protocol::{
    CatalogItem, Connection, FrameMode, Message, PreviewStats, ProofMode, Role, SourceUnit,
};
use crate::ui::{
    DEV_EDITOR, DEV_NEXT, DEV_PREVIOUS, DEV_RESET, DEV_RUN, DEV_TEST, DevFrameState, dev_frame,
};
use crate::view::RetainedView;

pub fn connect(path: &Path) -> Result<Connection, Box<dyn std::error::Error + Send + Sync>> {
    Ok(Connection::connect(path, Role::Dev)?)
}

#[derive(Default)]
struct DevUiUpdate {
    title: Option<String>,
    source: bool,
    scroll: bool,
    status: bool,
    perf: Option<String>,
    interaction: bool,
    resized: bool,
}

impl DevUiUpdate {
    fn record(&mut self, change: DevChange) {
        match change {
            DevChange::None => {}
            DevChange::Interaction => self.interaction = true,
            DevChange::Scroll => self.scroll = true,
            DevChange::SourceAndStatus => {
                self.source = true;
                self.status = true;
            }
        }
    }

    fn patches(&self, state: &DevState) -> Vec<DocumentPatch> {
        let mut patches = Vec::with_capacity(5);
        if let Some(title) = &self.title {
            patches.push(text_patch("dev.example", title));
        }
        if self.source {
            patches.push(text_patch(DEV_EDITOR, state.source()));
        }
        if self.scroll {
            patches.push(DocumentPatch::SetScroll {
                id: DocumentNodeId(DEV_EDITOR.to_owned()),
                scroll: ScrollState {
                    x: 0.0,
                    y: state.editor_scroll(),
                },
            });
        }
        if self.status {
            patches.push(text_patch("dev.status", state.status()));
        }
        if let Some(perf) = &self.perf {
            patches.push(text_patch("dev.perf", perf));
        }
        patches
    }
}

fn text_patch(id: &str, text: &str) -> DocumentPatch {
    DocumentPatch::SetText {
        id: DocumentNodeId(id.to_owned()),
        text: TextValue {
            text: text.to_owned(),
        },
    }
}

pub async fn run(mut host: NativeSurfaceHost, mut writer: Connection) -> NativeRoleResult {
    let observer = ObserverClient::from_env()?;
    let mut product = ProductFrame::attach(&mut host, ObserverRole::Dev).await?;
    emit(
        &observer,
        ObserverEvent::RoleMetadata(product.role_metadata()),
    );
    let mut columns = boon_native_gpu::GlyphonRenderTextColumnMeasurer::new();
    let mut view = RetainedView::new(
        dev_frame(DevFrameState {
            example_label: "Boon",
            source_path: "no source",
            source: "",
            editor_scroll: 0.0,
            status: "Waiting for source...",
            perf: "Preview idle, proof off",
        }),
        viewport(&host),
        &mut columns,
    )?;
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

    let mut state = DevState::new(String::new());
    let mut catalog = Vec::<CatalogItem>::new();
    let mut active_id = String::new();
    let mut units = Vec::<SourceUnit>::new();
    let mut editable_index = 0usize;
    let mut revision = 0u64;
    let mut request_id = 0u64;
    let mut observed_targets = BTreeMap::<String, (f32, f32)>::new();
    loop {
        enum Wake {
            Native(Result<HostEventEnvelope, boon_native_app_window::NativeHostError>),
            Ipc(Option<Result<Message, String>>),
        }
        let wake = {
            let native = host.next_event().fuse();
            let command = incoming.next().fuse();
            pin_mut!(native, command);
            select! {
                event = native => Wake::Native(event),
                message = command => Wake::Ipc(message),
            }
        };
        let mut transaction = NativeFrameTransaction::default();
        let mut update = DevUiUpdate::default();
        match wake {
            Wake::Native(event) => {
                for accepted in drain_native_events(&mut host, event).await? {
                    let envelope = &accepted.envelope;
                    let target_name = native_target_name(&view, &envelope.event, &state);
                    let visible_input = if matches!(envelope.event, HostEvent::Resize(_)) {
                        view.resize(viewport(&host), &mut columns)?;
                        update.resized = true;
                        true
                    } else {
                        let result = state.handle_event(&envelope.event, |x, y| {
                            view.hit(x, y).map(str::to_owned)
                        });
                        update.record(result.change);
                        match result.action {
                            DevAction::None => {}
                            DevAction::Previous | DevAction::Next => {
                                if let Some(id) = adjacent_id(
                                    &catalog,
                                    &active_id,
                                    result.action == DevAction::Next,
                                ) {
                                    writer.send(&Message::DevSelectExample { example_id: id })?;
                                    state.set_status("Opening example...");
                                    update.status = true;
                                }
                            }
                            DevAction::Run => {
                                update_editable_unit(&state, &mut units, editable_index);
                                revision = revision.saturating_add(1);
                                writer.send(&Message::DevRun {
                                    revision,
                                    units: units.clone(),
                                })?;
                                state.set_status("Compiling...");
                                update.status = true;
                            }
                            DevAction::Reset => {
                                writer.send(&Message::DevReset)?;
                                state.set_status("Resetting...");
                                update.status = true;
                            }
                            DevAction::Test => {
                                update_editable_unit(&state, &mut units, editable_index);
                                revision = revision.saturating_add(1);
                                request_id = request_id.saturating_add(1);
                                writer.send(&Message::DevTest {
                                    request_id,
                                    revision,
                                    units: units.clone(),
                                })?;
                                state.set_status("TEST running in preview...");
                                update.status = true;
                            }
                            DevAction::Close => {
                                observe_input(&observer, envelope, target_name, false);
                                let _ = writer.send(&Message::Shutdown);
                                return Ok(());
                            }
                        }
                        result.visible_change()
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
                        active_id: next_active,
                    } => {
                        catalog = entries;
                        active_id = next_active;
                    }
                    Message::OpenEditor {
                        example_id,
                        label,
                        revision: next_revision,
                        units: next_units,
                    } => {
                        active_id = example_id;
                        revision = next_revision;
                        units = next_units;
                        editable_index = units.len().saturating_sub(1);
                        state.replace_source(
                            units
                                .get(editable_index)
                                .map(|unit| unit.source.clone())
                                .unwrap_or_default(),
                        );
                        let source_path = units
                            .get(editable_index)
                            .map(|unit| unit.path.as_str())
                            .unwrap_or("no source");
                        update.title = Some(format!("{label}  {source_path}"));
                        update.source = true;
                        update.scroll = true;
                        update.status = true;
                        update.interaction = true;
                    }
                    Message::PreviewStats(stats) => {
                        update.perf = Some(perf_line(&stats));
                    }
                    Message::PreviewStatus {
                        revision: status_revision,
                        ok,
                        message,
                    } => {
                        state.set_status(format!(
                            "r{status_revision} {}: {message}",
                            if ok { "ready" } else { "error" }
                        ));
                        update.status = true;
                    }
                    Message::PreviewTestResult {
                        request_id: completed,
                        passed,
                        message,
                    } => {
                        state.set_status(format!(
                            "TEST #{completed} {}: {message}",
                            if passed { "passed" } else { "failed" }
                        ));
                        update.status = true;
                    }
                    Message::Ready {
                        role: Role::Preview,
                    } => {
                        state.set_status("Preview connected");
                        update.status = true;
                    }
                    Message::Shutdown => return Ok(()),
                    other => {
                        return Err(format!("invalid desktop-to-dev message: {other:?}").into());
                    }
                }
            }
        }

        let mut render_changed = update.resized;
        let patches = update.patches(&state);
        if !patches.is_empty() {
            let document_update = view.apply_patches(patches, &mut columns)?;
            render_changed |= document_update.render_changed || document_update.layout_changed;
        }
        if update.interaction {
            let interaction_update = view.set_interaction_state(
                state.hovered(),
                state.editor_focused().then_some(crate::ui::DEV_EDITOR),
                &mut columns,
            )?;
            render_changed |=
                interaction_update.render_changed || interaction_update.layout_changed;
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
        DEV_EDITOR,
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
        HostEvent::Keyboard(_) | HostEvent::TextInput(_) | HostEvent::Ime(_)
            if state.editor_focused() =>
        {
            Some(crate::ui::DEV_EDITOR.to_owned())
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

fn update_editable_unit(state: &DevState, units: &mut [SourceUnit], index: usize) {
    if let Some(unit) = units.get_mut(index) {
        unit.source = state.source().to_owned();
    }
}

fn adjacent_id(entries: &[CatalogItem], active: &str, next: bool) -> Option<String> {
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
    format!(
        "Preview {mode}, last {:.2}ms, render {:.2}ms, age {}ms, proof {proof}, misses {}, drops {}",
        f64::from(stats.input_to_present_micros) / 1000.0,
        f64::from(stats.render_micros) / 1000.0,
        stats.sample_age_millis,
        stats.missed_frames,
        stats.dropped_snapshots,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use boon_document::render_scene::ApproximateTextColumnMeasurer;
    use boon_host::{PointerButton, PointerEvent, PointerPhase, SurfaceId, WheelEvent};

    fn pointer(x: f32, y: f32, phase: PointerPhase) -> HostEvent {
        HostEvent::Pointer(PointerEvent {
            surface: SurfaceId("dev".to_owned()),
            x,
            y,
            phase,
            button: (phase != PointerPhase::Move).then_some(PointerButton::Primary),
        })
    }

    fn apply_state_event(
        state: &mut DevState,
        view: &mut RetainedView,
        columns: &mut ApproximateTextColumnMeasurer,
        event: &HostEvent,
    ) -> (crate::dev_state::DevEventResult, usize) {
        let result = state.handle_event(event, |x, y| view.hit(x, y).map(str::to_owned));
        let mut update = DevUiUpdate::default();
        update.record(result.change);
        let patches = update.patches(state);
        let patch_count = patches.len();
        if !patches.is_empty() {
            view.apply_patches(patches, columns).unwrap();
        }
        if update.interaction {
            view.set_interaction_state(
                state.hovered(),
                state.editor_focused().then_some(DEV_EDITOR),
                columns,
            )
            .unwrap();
        }
        (result, patch_count)
    }

    #[test]
    fn example_navigation_wraps_without_special_example_names() {
        let entries = vec![
            CatalogItem {
                id: "a".into(),
                label: "A".into(),
            },
            CatalogItem {
                id: "b".into(),
                label: "B".into(),
            },
        ];
        assert_eq!(adjacent_id(&entries, "a", false).as_deref(), Some("b"));
        assert_eq!(adjacent_id(&entries, "b", true).as_deref(), Some("a"));
    }

    #[test]
    fn large_source_hover_test_and_wheel_stay_on_retained_dev_path() {
        let source = "value: 1234567890\n".repeat(3_000);
        assert!(source.len() >= 50_000);
        let mut state = DevState::new(source);
        let mut columns = ApproximateTextColumnMeasurer;
        let mut view = RetainedView::new(
            dev_frame(DevFrameState {
                example_label: "Counter",
                source_path: "examples/counter.bn",
                source: state.source(),
                editor_scroll: 0.0,
                status: state.status(),
                perf: "Preview idle, proof off",
            }),
            Viewport {
                surface: 1,
                width: 1_160.0,
                height: 820.0,
                scale: 1.0,
            },
            &mut columns,
        )
        .unwrap();
        let initial_full_lowers = view.retained_stats().full_lower_count;
        let test = view.target_for_source(DEV_TEST, None).expect("TEST target");
        let next = view.target_for_source(DEV_NEXT, None).expect("Next target");
        let mut observed_targets = BTreeMap::new();
        emit_dev_targets(&None, &view, &mut observed_targets);
        assert!(observed_targets.contains_key(DEV_EDITOR));

        for index in 0..200 {
            let target = if index % 2 == 0 { &test } else { &next };
            let (_, patch_count) = apply_state_event(
                &mut state,
                &mut view,
                &mut columns,
                &pointer(target.center_x, target.center_y, PointerPhase::Move),
            );
            assert_eq!(patch_count, 0, "hover must not clone or patch source text");
        }

        apply_state_event(
            &mut state,
            &mut view,
            &mut columns,
            &pointer(test.center_x, test.center_y, PointerPhase::Down),
        );
        let (result, patch_count) = apply_state_event(
            &mut state,
            &mut view,
            &mut columns,
            &pointer(test.center_x, test.center_y, PointerPhase::Up),
        );
        assert_eq!(result.action, DevAction::Test);
        assert_eq!(patch_count, 0);

        let editor = view
            .target_for_source(DEV_EDITOR, None)
            .expect("editor target");
        apply_state_event(
            &mut state,
            &mut view,
            &mut columns,
            &pointer(editor.center_x, editor.center_y, PointerPhase::Down),
        );
        assert!(view.scene().visual_primitives.iter().any(|primitive| {
            primitive.node.0 == DEV_EDITOR
                && primitive.primitive == boon_document::RenderVisualPrimitiveKind::Border
        }));
        let (_, patch_count) = apply_state_event(
            &mut state,
            &mut view,
            &mut columns,
            &HostEvent::Wheel(WheelEvent {
                surface: SurfaceId("dev".to_owned()),
                x: editor.center_x,
                y: editor.center_y,
                delta_x: 0.0,
                delta_y: 120.0,
            }),
        );
        assert_eq!(patch_count, 1, "wheel must emit one retained scroll patch");
        assert_eq!(state.editor_scroll(), 120.0);
        assert_eq!(view.retained_stats().full_lower_count, initial_full_lowers);
    }
}
