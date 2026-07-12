use std::fs;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::os::unix::net::UnixListener;
#[cfg(target_os = "linux")]
use std::process::{Command, ExitStatus, Stdio};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Arc, mpsc};
#[cfg(target_os = "linux")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "linux")]
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[cfg(target_os = "linux")]
use crate::observer::{
    FrameEvidenceKey, FramePresented, InputAccepted, InputKind, OBSERVER_SOCKET_ENV, ObserverEvent,
    ObserverRole, PROOF_ARTIFACT_DIR_ENV, PROOF_MODE_ENV, PROOF_SAMPLE_ORDINAL_ENV, ProofArtifact,
    RoleMetadata, read_event,
};
#[cfg(target_os = "linux")]
use crate::{native_input::NativeInput, ui::DEV_EDITOR, workspace_control::WorkspaceGuard};

const FORMAT_VERSION: u16 = 2;
const PROTOCOL: &str = "boon-gate-evidence-v2";
const MAX_DETAIL_BYTES: usize = 1_000;
#[cfg(target_os = "linux")]
const ROLE_READY_TIMEOUT: Duration = Duration::from_secs(45);
#[cfg(target_os = "linux")]
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(target_os = "linux")]
const NORMAL_VISIBLE_SAMPLE_COUNT: usize = 120;
#[cfg(target_os = "linux")]
const INPUT_CALIBRATION_QUIET: Duration = Duration::from_millis(20);
#[cfg(target_os = "linux")]
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(target_os = "linux")]
const MAX_OBSERVER_EVENTS: usize = 8_192;
#[cfg(target_os = "linux")]
const OBSERVER_QUEUE_DEPTH: usize = 8_192;
#[cfg(target_os = "linux")]
const NATIVE_WORKSPACE: &str = "boon-circuit";

pub fn run(args: &[String]) -> Result<(), String> {
    let gate = Gate::parse(required(args, "--gate")?)?;
    let output = PathBuf::from(required(args, "--evidence-output")?);
    let artifact_dir = PathBuf::from(required(args, "--artifact-dir")?);
    let run_id = required(args, "--run-id")?.to_owned();
    let source_digest = required(args, "--source-digest")?.to_owned();
    fs::create_dir_all(&artifact_dir).map_err(|error| {
        format!(
            "create verifier artifact directory {}: {error}",
            artifact_dir.display()
        )
    })?;

    let evidence = if gate.is_timed() {
        run_native_harness(gate, &run_id, &artifact_dir)
    } else {
        negative_evidence()
    };
    let envelope = ProducerEnvelope {
        format: FORMAT_VERSION,
        protocol: PROTOCOL,
        gate: gate.slug(),
        run_id,
        source_digest,
        evidence,
    };
    write_envelope(&output, &envelope)
}

fn negative_evidence() -> GateEvidence {
    let mut invalid = vec![0_u8; 4];
    invalid.copy_from_slice(&(u32::MAX).to_le_bytes());
    #[cfg(target_os = "linux")]
    let rejected = read_event(&mut invalid.as_slice()).is_err();
    #[cfg(not(target_os = "linux"))]
    let rejected = true;
    GateEvidence {
        checks: vec![if rejected {
            Check::pass(
                "negative-bounded-observer-frame",
                "the verifier-only binary observer rejects an oversized frame before allocation",
            )
        } else {
            Check::fail(
                "negative-bounded-observer-frame",
                "the verifier-only binary observer accepted an oversized frame",
            )
        }],
        producer: None,
        native: None,
        product_ux_timings: Vec::new(),
        async_proof_timing: None,
        artifacts: Vec::new(),
    }
}

fn run_native_harness(gate: Gate, run_id: &str, artifact_dir: &Path) -> GateEvidence {
    #[cfg(target_os = "linux")]
    {
        run_linux_harness(gate, run_id, artifact_dir)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (gate, run_id, artifact_dir);
        GateEvidence::failed(Check::fail(
            "native-os-input-harness",
            "the kernel virtual-input harness is available only on Linux",
        ))
    }
}

#[cfg(target_os = "linux")]
fn run_linux_harness(gate: Gate, run_id: &str, artifact_dir: &Path) -> GateEvidence {
    let mut capture = Capture::default();
    capture.checks.push(product_scheduler_check());
    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            capture.checks.push(Check::fail(
                "workspace-discovery",
                format!("cannot read verifier working directory: {error}"),
            ));
            return capture.into_evidence(gate);
        }
    };
    let scratch = match ScratchDir::create(run_id, gate.slug()) {
        Ok(scratch) => scratch,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-os-input-scratch", error));
            return capture.into_evidence(gate);
        }
    };

    let observer_path = scratch.path.join("observer.sock");
    let mut observer = match ObserverServer::bind(&observer_path) {
        Ok(server) => server,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("verifier-observer-bind", error));
            return capture.into_evidence(gate);
        }
    };
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            capture.checks.push(Check::fail(
                "native-producer-executable",
                format!("cannot resolve native producer executable: {error}"),
            ));
            return capture.into_evidence(gate);
        }
    };
    let mut session = match NativeSession::start(
        &workspace,
        &scratch.path,
        &executable,
        gate.example_id(),
        &observer_path,
        artifact_dir,
    ) {
        Ok(session) => session,
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-os-input-session", error));
            return capture.into_evidence(gate);
        }
    };
    capture.checks.push(Check::pass(
        "kernel-virtual-input",
        format!(
            "uinput pointer and keyboard are owned by launch-scoped seat {}",
            session.isolated_seat_name
        ),
    ));
    capture.checks.push(Check::pass(
        "regular-cosmic-wayland",
        "preview and dev use ordinary Wayland/app_window callbacks on a launch-scoped COSMIC seat",
    ));

    let roles = match session.wait_for_roles(ROLE_READY_TIMEOUT) {
        Ok(roles) => {
            capture.checks.push(Check::pass(
                "native-role-processes",
                format!(
                    "desktop pid {}, preview pid {}, dev pid {} are distinct live processes",
                    session.desktop_id(),
                    roles.preview,
                    roles.dev
                ),
            ));
            roles
        }
        Err(error) => {
            capture
                .checks
                .push(Check::fail("native-role-processes", error));
            capture.checks.push(cleanup_check(session.shutdown()));
            return capture.into_evidence(gate);
        }
    };

    if let Err(error) = session.prepare_background_workspace(&executable) {
        capture
            .checks
            .push(Check::fail("isolated-cosmic-workspace", error));
        capture.checks.push(cleanup_check(session.shutdown()));
        return capture.into_evidence(gate);
    }
    capture.checks.push(Check::pass(
        "isolated-cosmic-workspace",
        "the bounded test workspace remained inactive while launch-scoped input targeted it",
    ));

    let exercise = exercise_native_roles(
        gate,
        &mut session,
        &mut observer,
        &mut capture.events,
        &mut capture.samples,
    );
    match exercise {
        Ok(()) => capture.checks.push(Check::pass(
            "real-native-scenario",
            "kernel virtual devices clicked dev TEST, exercised preview input, and completed bounded samples through COSMIC",
        )),
        Err(error) => capture
            .checks
            .push(Check::fail("real-native-scenario", error)),
    }

    drain_events(
        &mut observer,
        &mut capture.events,
        Duration::from_millis(300),
    );
    if process_exists(session.desktop_id())
        && process_exists(roles.preview)
        && process_exists(roles.dev)
    {
        capture.checks.push(Check::pass(
            "native-role-liveness-after-input",
            "desktop, preview, and dev remained live after the real Wayland input sequence",
        ));
    } else {
        capture.checks.push(Check::fail(
            "native-role-liveness-after-input",
            format!(
                "a native role exited after input; desktop={}, preview={}, dev={}",
                process_exists(session.desktop_id()),
                process_exists(roles.preview),
                process_exists(roles.dev)
            ),
        ));
    }
    capture.checks.push(cleanup_check(session.shutdown()));
    drain_events(
        &mut observer,
        &mut capture.events,
        Duration::from_millis(100),
    );
    capture.finalize_checks(gate, roles);
    capture.into_evidence(gate)
}

#[cfg(target_os = "linux")]
fn product_scheduler_check() -> Check {
    let policy = unsafe { libc::sched_getscheduler(0) };
    let nice = unsafe { libc::getpriority(libc::PRIO_PROCESS, 0) };
    check_result(
        "normal-product-scheduler",
        policy == libc::SCHED_OTHER && nice == 0,
        "native evidence producer and product children use SCHED_OTHER at nice 0",
        format!("native evidence producer scheduler policy={policy}, nice={nice}"),
    )
}

#[cfg(target_os = "linux")]
fn exercise_native_roles(
    gate: Gate,
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    samples: &mut ProductSamples,
) -> Result<(), String> {
    wait_for_metadata(observer, events)
        .map_err(|error| format!("preview/dev metadata did not become ready: {error}"))?;
    wait_for_value(observer, events, EVENT_TIMEOUT, 0, |event| match event {
        ObserverEvent::SourceSwitchFinal { .. } => Some(Ok(())),
        ObserverEvent::SourceFailed {
            revision,
            stage,
            message,
        } => Some(Err(format!(
            "preview source revision {revision} failed during {stage}: {message}"
        ))),
        _ => None,
    })
    .map_err(|error| format!("initial preview source did not become ready: {error}"))??;
    let mut placements = discover_window_placements(session, observer, events)?;
    let mut dev_placement = activate_window(
        session,
        observer,
        events,
        &mut placements,
        ObserverRole::Dev,
    )?;
    let dev_test_center = wait_for_value(observer, events, EVENT_TIMEOUT, 0, |event| match event {
        ObserverEvent::RoleTarget {
            role: ObserverRole::Dev,
            node,
            x,
            y,
        } if node == "dev.test" => Some((*x, *y)),
        _ => None,
    })
    .map_err(|error| format!("dev TEST target was not published: {error}"))?;
    let dev_editor_center =
        wait_for_value(observer, events, EVENT_TIMEOUT, 0, |event| match event {
            ObserverEvent::RoleTarget {
                role: ObserverRole::Dev,
                node,
                x,
                y,
            } if node == DEV_EDITOR => Some((*x, *y)),
            _ => None,
        })
        .map_err(|error| format!("dev editor target was not published: {error}"))?;

    let editor_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Dev,
        DEV_EDITOR,
        dev_editor_center,
        translated_target_candidates(
            dev_placement.origin,
            dev_editor_center.0,
            dev_editor_center.1,
        ),
    )?;
    let dev_input_start = events.len();
    session.run_driver(&[
        "move",
        &editor_point.0.to_string(),
        &editor_point.1.to_string(),
    ])?;
    session.run_driver(&["click", "left"])?;
    session.run_driver(&["key", "down", "left"])?;
    session.run_driver(&["key", "up", "left"])?;
    session.run_driver(&["axis", "vertical", "4"])?;
    session.run_driver(&["axis", "vertical", "-4"])?;
    wait_for_event(observer, events, EVENT_TIMEOUT, dev_input_start, |event| {
        matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Keyboard)
    })
    .map_err(|error| format!("dev editor did not accept real keyboard input: {error}"))?;
    wait_for_event(observer, events, EVENT_TIMEOUT, dev_input_start, |event| {
        matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Wheel)
    })
    .map_err(|error| format!("dev editor did not accept real wheel input: {error}"))?;

    let test_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Dev,
        "dev.test",
        dev_test_center,
        translated_target_candidates(dev_placement.origin, dev_test_center.0, dev_test_center.1),
    )
    .map_err(|error| format!("{error}; calibrated_dev={dev_placement:?}"))?;
    let before_test = events.len();
    session.run_driver(&["move", &test_point.0.to_string(), &test_point.1.to_string()])?;
    session.run_driver(&["click", "left"])?;
    let test_action_visible = wait_for_value(
        observer,
        events,
        EVENT_TIMEOUT,
        before_test,
        |event| match event {
            ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.target.as_deref() == Some("dev.test")
                    && input.kind == InputKind::PointerButton
                    && input.pointer_button_pressed == Some(false) =>
            {
                Some(input.visible_change)
            }
            _ => None,
        },
    )
    .map_err(|error| {
        format!(
            "dev TEST click was not accepted: {error}; observed={}",
            input_event_trace(events, before_test, 8)
        )
    })?;
    if !test_action_visible {
        return Err(format!(
            "dev TEST release reached the button but did not activate it; observed={}",
            input_event_trace(events, before_test, 8)
        ));
    }
    let test_target = wait_for_value(observer, events, EVENT_TIMEOUT, before_test, |event| {
        match event {
            ObserverEvent::TestTarget {
                request_id,
                node,
                source_path,
                x,
                y,
            } => Some(Ok((*request_id, node.clone(), source_path.clone(), *x, *y))),
            ObserverEvent::TestCompleted {
                request_id,
                passed: false,
                completed_steps,
                message,
            } => Some(Err(format!(
                "preview TEST request {request_id} failed after {completed_steps} steps: {message}"
            ))),
            _ => None,
        }
    })
    .map_err(|error| format!("preview did not publish a TEST result: {error}"))??;
    let preview_placement = activate_window(
        session,
        observer,
        events,
        &mut placements,
        ObserverRole::Preview,
    )?;
    let completion =
        wait_for_value(
            observer,
            events,
            EVENT_TIMEOUT,
            before_test,
            |event| match event {
                ObserverEvent::TestCompleted {
                    request_id,
                    passed,
                    completed_steps,
                    message,
                } if *request_id == test_target.0 => {
                    Some((*passed, *completed_steps, message.clone()))
                }
                _ => None,
            },
        )
        .map_err(|error| format!("preview TEST did not complete: {error}"))?;
    if !completion.0 {
        return Err(format!(
            "preview TEST failed after {} steps: {}",
            completion.1, completion.2
        ));
    }
    drain_events(observer, events, Duration::from_millis(250));

    let preview_candidates =
        translated_target_candidates(preview_placement.origin, test_target.3, test_target.4);
    let preview_point = locate_target(
        session,
        observer,
        events,
        ObserverRole::Preview,
        &test_target.1,
        (test_target.3, test_target.4),
        preview_candidates,
    )?;
    let off_target = if gate == Gate::CounterDev {
        None
    } else {
        Some(locate_different_preview_target(
            session,
            observer,
            events,
            &test_target.1,
            (gate == Gate::Cells).then_some(test_target.2.as_str()),
            preview_point,
            preview_placement.origin,
        )?)
    };
    for ordinal in 0..NORMAL_VISIBLE_SAMPLE_COUNT {
        let sequence = if gate == Gate::CounterDev {
            drive_click_sample(session, observer, events, preview_point, &test_target.1)
        } else {
            let point = if ordinal % 2 == 0 {
                preview_point
            } else {
                off_target.as_ref().expect("non-Counter hover target").0
            };
            drive_visible_sample(
                session,
                observer,
                events,
                point,
                InputKind::PointerMove,
                |input| {
                    if ordinal % 2 == 0 {
                        input.target.as_deref() == Some(test_target.1.as_str())
                    } else {
                        input.target.as_deref()
                            == off_target
                                .as_ref()
                                .expect("non-Counter hover target")
                                .1
                                .as_deref()
                    }
                },
            )
        }
        .map_err(|error| format!("preview sample {ordinal} failed: {error}"))?;
        samples.visible.insert(sequence);
    }
    if samples.visible.len() < NORMAL_VISIBLE_SAMPLE_COUNT {
        return Err("serialized preview interaction samples were not retained".to_owned());
    }

    if gate == Gate::Cells {
        let alternate = off_target.as_ref().expect("Cells alternate target");
        for ordinal in 0..24 {
            let (point, node) = if ordinal % 2 == 0 {
                (preview_point, test_target.1.as_str())
            } else {
                (
                    alternate.0,
                    alternate
                        .1
                        .as_deref()
                        .expect("Cells alternate has a same-route target"),
                )
            };
            let sequence = drive_click_sample(session, observer, events, point, node)
                .map_err(|error| format!("Cells selection sample {ordinal} failed: {error}"))?;
            samples.visible.insert(sequence);
            samples.clicks.insert(sequence);
        }
    }

    if gate == Gate::Cells {
        let move_start = events.len();
        session.run_driver(&[
            "move",
            &preview_point.0.to_string(),
            &preview_point.1.to_string(),
        ])?;
        wait_for_visible_present(
            observer,
            events,
            move_start,
            InputKind::PointerMove,
            |input| input.target.as_deref() == Some(test_target.1.as_str()),
        )
        .map_err(|error| {
            format!(
                "preview scroll target was not entered: {error}; observed={}",
                input_event_trace(events, move_start, 8)
            )
        })?;
        for ordinal in 0..148 {
            let amount = if ordinal % 2 == 0 { "-4" } else { "4" };
            let start = events.len();
            session.run_driver(&["axis", "vertical", amount])?;
            let sequence =
                wait_for_visible_present(observer, events, start, InputKind::Wheel, |_| true)
                    .map_err(|error| {
                        format!(
                            "scroll sample {ordinal} failed: {error}; observed={}",
                            input_event_trace(events, start, 8)
                        )
                    })?;
            samples.scroll.insert(sequence);
        }
        if samples.scroll.len() < 148 {
            return Err("serialized preview scroll samples were not retained".to_owned());
        }
    }

    if gate == Gate::CounterDev {
        let mut revision = maximum_switch_revision(events);
        for ordinal in 0..23 {
            session.reconcile_background_layout()?;
            dev_placement = activate_window(
                session,
                observer,
                events,
                &mut placements,
                ObserverRole::Dev,
            )?;
            let target = if ordinal % 2 == 0 {
                "dev.next"
            } else {
                "dev.previous"
            };
            let center = observed_role_target(events, ObserverRole::Dev, target)
                .ok_or_else(|| format!("{target} retained hit center was not observed"))?;
            locate_target(
                session,
                observer,
                events,
                ObserverRole::Dev,
                target,
                center,
                translated_target_candidates(dev_placement.origin, center.0, center.1),
            )?;
            let start = events.len();
            session.run_driver(&["click", "left"])?;
            revision = wait_for_value(
                observer,
                events,
                EVENT_TIMEOUT,
                start,
                |event| match event {
                    ObserverEvent::SourceSwitchFinal { revision: next, .. } if *next > revision => {
                        Some(*next)
                    }
                    _ => None,
                },
            )
            .map_err(|error| {
                format!(
                    "Counter source switch {ordinal} did not finish: {error}; observed={}",
                    input_event_trace(events, start, 12)
                )
            })?;
        }
    }

    wait_for_event(observer, events, EVENT_TIMEOUT, 0, |event| {
        matches!(
            event,
            ObserverEvent::ProofCompleted {
                artifact: Some(_),
                error: None,
                ..
            }
        )
    })
    .map_err(|error| format!("final app-owned proof did not complete: {error}"))?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn input_event_trace(events: &[ObserverEvent], start: usize, limit: usize) -> String {
    let mut values = events
        .iter()
        .skip(start.min(events.len()))
        .filter_map(|event| match event {
            ObserverEvent::InputAccepted(input) => Some(format!(
                "{:?}:{:?}:{:?}@({:.1},{:.1}) target={:?} visible={}",
                input.role,
                input.kind,
                input.pointer_button_pressed,
                input.pointer_x.unwrap_or_default(),
                input.pointer_y.unwrap_or_default(),
                input.target,
                input.visible_change
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    if values.len() > limit {
        values.drain(..values.len() - limit);
    }
    format!("{values:?}")
}

#[cfg(target_os = "linux")]
fn drive_click_sample(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    target: &str,
) -> Result<u64, String> {
    drain_events(observer, events, INPUT_CALIBRATION_QUIET);
    let start = events.len();
    session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
    session.run_driver(&["click", "left"])?;
    wait_for_visible_present(observer, events, start, InputKind::PointerButton, |input| {
        input.target.as_deref() == Some(target) && input.pointer_button_pressed == Some(false)
    })
}

#[cfg(target_os = "linux")]
fn drive_visible_sample(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    kind: InputKind,
    matches_input: impl Fn(&InputAccepted) -> bool,
) -> Result<u64, String> {
    let start = events.len();
    session.run_driver(&["move", &point.0.to_string(), &point.1.to_string()])?;
    wait_for_visible_present(observer, events, start, kind, matches_input)
}

#[cfg(target_os = "linux")]
fn wait_for_visible_present(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    start: usize,
    kind: InputKind,
    matches_input: impl Fn(&InputAccepted) -> bool,
) -> Result<u64, String> {
    let sequence =
        wait_for_value(
            observer,
            events,
            Duration::from_secs(2),
            start,
            |event| match event {
                ObserverEvent::InputAccepted(input)
                    if input.role == ObserverRole::Preview
                        && input.real_os
                        && input.kind == kind
                        && input.visible_change
                        && matches_input(input) =>
                {
                    Some(input.event_sequence)
                }
                _ => None,
            },
        )
        .map_err(|error| format!("visible input was not accepted: {error}"))?;
    wait_for_event(observer, events, Duration::from_secs(2), start, |event| {
        matches!(event, ObserverEvent::FramePresented(frame)
                if frame.role == ObserverRole::Preview
                    && frame.event_sequence == Some(sequence))
    })
    .map_err(|error| format!("accepted input {sequence} was not presented: {error}"))?;
    Ok(sequence)
}

#[cfg(target_os = "linux")]
fn wait_for_metadata(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
) -> Result<(), String> {
    wait_for_count(observer, events, ROLE_READY_TIMEOUT, |events| {
        let preview = events.iter().any(|event| {
            matches!(event, ObserverEvent::RoleMetadata(metadata)
                if metadata.role == ObserverRole::Preview)
        });
        let dev = events.iter().any(|event| {
            matches!(event, ObserverEvent::RoleMetadata(metadata)
                if metadata.role == ObserverRole::Dev)
        });
        preview && dev
    })
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug)]
struct WindowPlacement {
    origin: (i32, i32),
    visible_point: (i32, i32),
}

#[cfg(target_os = "linux")]
fn discover_window_placements(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
) -> Result<BTreeMap<ObserverRole, WindowPlacement>, String> {
    let mut placements = BTreeMap::new();
    let (first_role, _) = observe_window(session, observer, events, None)?;
    let first_placement = stable_window_placement(session, observer, events, first_role)?;
    placements.insert(first_role, first_placement);
    let second_role = other_role(first_role);
    let second_placement = stable_window_placement(session, observer, events, second_role)?;
    placements.insert(second_role, second_placement);
    Ok(placements)
}

#[cfg(target_os = "linux")]
fn stable_window_placement(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    expected: ObserverRole,
) -> Result<WindowPlacement, String> {
    let mut previous: Option<(i32, i32)> = None;
    for _ in 0..8 {
        let (_, placement) = observe_window(session, observer, events, Some(expected))?;
        if previous.is_some_and(|previous| {
            (previous.0 - placement.origin.0).abs() <= 1
                && (previous.1 - placement.origin.1).abs() <= 1
        }) {
            return Ok(placement);
        }
        previous = Some(placement.origin);
    }
    Err(format!(
        "{expected:?} native window placement did not stabilize; last origin={previous:?}"
    ))
}

#[cfg(target_os = "linux")]
fn activate_window(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    placements: &mut BTreeMap<ObserverRole, WindowPlacement>,
    expected: ObserverRole,
) -> Result<WindowPlacement, String> {
    let placement = match placements.get(&expected).copied() {
        Some(placement)
            if role_at_point(session, observer, events, placement.visible_point)? == expected =>
        {
            placement
        }
        _ => stable_window_placement(session, observer, events, expected)?,
    };
    placements.insert(expected, placement);
    Ok(placement)
}

#[cfg(target_os = "linux")]
fn role_at_point(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
) -> Result<ObserverRole, String> {
    move_with_marker(session, observer, events, point, Duration::from_millis(400))
        .or_else(|_| {
            move_with_marker(
                session,
                observer,
                events,
                (point.0.saturating_add(1), point.1),
                Duration::from_millis(400),
            )
        })
        .map(|(_, input)| input.role)
}

#[cfg(target_os = "linux")]
fn move_with_marker(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    point: (i32, i32),
    timeout: Duration,
) -> Result<((i32, i32), InputAccepted), String> {
    drain_events(observer, events, INPUT_CALIBRATION_QUIET);
    let start = events.len();
    let actual = session.move_pointer(point)?;
    let marker = wait_for_value(observer, events, timeout, start, |event| match event {
        ObserverEvent::InputAccepted(input)
            if input.real_os
                && input.kind == InputKind::PointerMove
                && input.pointer_x.is_some()
                && input.pointer_y.is_some() =>
        {
            Some(input.clone())
        }
        _ => None,
    })
    .map_err(|error| format!("pointer move marker was not accepted: {error}"))?;
    Ok((actual, marker))
}

#[cfg(target_os = "linux")]
fn observe_window(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    expected: Option<ObserverRole>,
) -> Result<(ObserverRole, WindowPlacement), String> {
    let mut last_role = None;
    let mut observations = VecDeque::with_capacity(12);
    for point in window_scan_candidates(session.pointer_space()?) {
        if let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(100))
        {
            last_role = Some(input.role);
            if observations.len() == 12 {
                observations.pop_front();
            }
            observations.push_back(format!(
                "requested={point:?} acknowledged={actual:?} role={:?} local=({:.1},{:.1})",
                input.role,
                input.pointer_x.unwrap_or_default(),
                input.pointer_y.unwrap_or_default()
            ));
            if expected.is_some() && expected != Some(input.role) {
                continue;
            }
            let local_x = input.pointer_x.expect("filtered pointer x");
            let local_y = input.pointer_y.expect("filtered pointer y");
            return Ok((
                input.role,
                WindowPlacement {
                    origin: (
                        actual.0 - local_x.round() as i32,
                        actual.1 - local_y.round() as i32,
                    ),
                    visible_point: actual,
                },
            ));
        }
    }
    match expected {
        Some(role) => Err(format!(
            "real pointer scan could not observe {role:?}; last visible role was {last_role:?}; observations={observations:?}"
        )),
        None => Err("real pointer input did not identify a native role".to_owned()),
    }
}

#[cfg(target_os = "linux")]
fn other_role(role: ObserverRole) -> ObserverRole {
    match role {
        ObserverRole::Preview => ObserverRole::Dev,
        ObserverRole::Dev => ObserverRole::Preview,
    }
}

#[cfg(target_os = "linux")]
fn window_scan_candidates((width, height): (i32, i32)) -> Vec<(i32, i32)> {
    let point = |x_num: i32, x_den: i32, y_num: i32, y_den: i32| {
        (
            (width.saturating_mul(x_num) / x_den).clamp(0, width.saturating_sub(1)),
            (height.saturating_mul(y_num) / y_den).clamp(0, height.saturating_sub(1)),
        )
    };
    let mut candidates = vec![
        point(1, 4, 1, 4),
        point(3, 4, 1, 4),
        point(1, 4, 3, 4),
        point(3, 4, 3, 4),
        point(1, 2, 1, 2),
        point(7, 8, 1, 2),
    ];
    for y_index in 1..10 {
        for x_index in 1..16 {
            let candidate = point(x_index, 16, y_index, 10);
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

#[cfg(target_os = "linux")]
fn observed_role_target(
    events: &[ObserverEvent],
    role: ObserverRole,
    expected_node: &str,
) -> Option<(f32, f32)> {
    events.iter().rev().find_map(|event| match event {
        ObserverEvent::RoleTarget {
            role: observed,
            node,
            x,
            y,
        } if *observed == role && node == expected_node => Some((*x, *y)),
        _ => None,
    })
}

#[cfg(target_os = "linux")]
fn locate_target(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    role: ObserverRole,
    target: &str,
    target_center: (f32, f32),
    candidates: Vec<(i32, i32)>,
) -> Result<(i32, i32), String> {
    let seed = candidates.iter().copied().take(4).collect::<Vec<_>>();
    let mut candidates = VecDeque::from(candidates);
    let mut observations = VecDeque::with_capacity(4);
    for _ in 0..32 {
        let Some(point) = candidates.pop_front() else {
            break;
        };
        let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(400))
        else {
            continue;
        };
        if observations.len() == 4 {
            observations.pop_front();
        }
        observations.push_back(format!(
            "global={actual:?} local=({:.1},{:.1}) role={:?} target={:?}",
            input.pointer_x.unwrap_or_default(),
            input.pointer_y.unwrap_or_default(),
            input.role,
            input.target
        ));
        if input.role != role {
            continue;
        }
        if input.target.as_deref() == Some(target) {
            return Ok(actual);
        }
        let local_x = input.pointer_x.expect("filtered pointer x");
        let local_y = input.pointer_y.expect("filtered pointer y");
        let corrected = (
            actual.0 + (target_center.0 - local_x).round() as i32,
            actual.1 + (target_center.1 - local_y).round() as i32,
        );
        if corrected != actual {
            candidates.push_front(corrected);
        }
    }
    Err(format!(
        "real pointer scan could not resolve {role:?} target `{target}` at local center ({:.1},{:.1}); seed={seed:?}; observations={observations:?}",
        target_center.0, target_center.1
    ))
}

#[cfg(target_os = "linux")]
fn locate_different_preview_target(
    session: &mut NativeSession,
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    target: &str,
    required_source_path: Option<&str>,
    around: (i32, i32),
    origin: (i32, i32),
) -> Result<((i32, i32), Option<String>), String> {
    let candidates = [
        (around.0 + 100, around.1),
        (around.0 - 100, around.1),
        (around.0, around.1 + 80),
        (origin.0 + 20, origin.1 + 20),
        (origin.0 + 760, origin.1 + 20),
        (origin.0 + 20, origin.1 + 560),
    ];
    for point in candidates {
        if let Ok((actual, input)) =
            move_with_marker(session, observer, events, point, Duration::from_millis(180))
            && input.role == ObserverRole::Preview
            && input.target.as_deref() != Some(target)
            && required_source_path
                .is_none_or(|source_path| input.target_source_path.as_deref() == Some(source_path))
        {
            return Ok((actual, input.target));
        }
    }
    Err("could not find a second preview hover state for real interaction samples".to_owned())
}

#[cfg(target_os = "linux")]
fn translated_target_candidates(origin: (i32, i32), x: f32, y: f32) -> Vec<(i32, i32)> {
    let base_x = origin.0 + x.round() as i32;
    let base_y = origin.1 + y.round() as i32;
    let mut candidates = Vec::new();
    for dy in [0, 24, 32, -8, 40, -16] {
        for dx in [0, -8, 8, -16, 16, -24, 24] {
            candidates.push((base_x + dx, base_y + dy));
        }
    }
    candidates
}

#[cfg(target_os = "linux")]
fn wait_for_event(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    start: usize,
    predicate: impl Fn(&ObserverEvent) -> bool,
) -> Result<(), String> {
    wait_for_value(observer, events, timeout, start, |event| {
        predicate(event).then_some(())
    })
}

#[cfg(target_os = "linux")]
fn wait_for_value<T>(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    start: usize,
    map: impl Fn(&ObserverEvent) -> Option<T>,
) -> Result<T, String> {
    let deadline = Instant::now() + timeout;
    let mut scanned = start.min(events.len());
    loop {
        while scanned < events.len() {
            if let Some(value) = map(&events[scanned]) {
                return Ok(value);
            }
            scanned += 1;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "observer condition was not met within {}ms",
                timeout.as_millis()
            ));
        }
        match observer.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => push_event(events, event)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("verifier observer disconnected".to_owned());
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn wait_for_count(
    observer: &mut ObserverServer,
    events: &mut Vec<ObserverEvent>,
    timeout: Duration,
    predicate: impl Fn(&[ObserverEvent]) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if predicate(events) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "observer sample count was not met within {}ms",
                timeout.as_millis()
            ));
        }
        match observer.recv_timeout(Duration::from_millis(50)) {
            Ok(event) => push_event(events, event)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("verifier observer disconnected".to_owned());
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn drain_events(observer: &mut ObserverServer, events: &mut Vec<ObserverEvent>, quiet: Duration) {
    while let Ok(event) = observer.recv_timeout(quiet) {
        if push_event(events, event).is_err() {
            return;
        }
    }
}

#[cfg(target_os = "linux")]
fn push_event(
    events: &mut Vec<ObserverEvent>,
    event: Result<ObserverEvent, String>,
) -> Result<(), String> {
    let event = event?;
    if events.len() >= MAX_OBSERVER_EVENTS {
        return Err(format!(
            "verifier observer exceeded its bounded {MAX_OBSERVER_EVENTS}-event capacity"
        ));
    }
    events.push(event);
    Ok(())
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct ProductSamples {
    visible: BTreeSet<u64>,
    clicks: BTreeSet<u64>,
    scroll: BTreeSet<u64>,
}

#[cfg(target_os = "linux")]
impl ProductSamples {
    fn callback_sequences(&self) -> BTreeSet<u64> {
        self.visible.union(&self.scroll).copied().collect()
    }
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct Capture {
    checks: Vec<Check>,
    events: Vec<ObserverEvent>,
    samples: ProductSamples,
}

#[cfg(target_os = "linux")]
impl Capture {
    fn finalize_checks(&mut self, gate: Gate, roles: RolePids) {
        let metadata = role_metadata(&self.events);
        let metadata_ok = metadata.contains_key(&ObserverRole::Preview)
            && metadata.contains_key(&ObserverRole::Dev)
            && metadata
                .get(&ObserverRole::Preview)
                .is_some_and(|value| value.pid == roles.preview)
            && metadata
                .get(&ObserverRole::Dev)
                .is_some_and(|value| value.pid == roles.dev);
        self.checks.push(check_result(
            "role-owned-native-metadata",
            metadata_ok,
            "preview and dev exported PID, adapter, surface, epoch, format, and present metadata",
            "preview/dev role metadata was missing or did not match the live role PIDs",
        ));

        let real_test = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.target.as_deref() == Some("dev.test")
                    && input.kind == InputKind::PointerButton)
        });
        let test_passed = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::TestCompleted { passed: true, completed_steps, .. }
                if *completed_steps > 0)
        });
        self.checks.push(check_result(
            "real-dev-test-click",
            real_test && test_passed,
            "real Wayland pointer input clicked dev TEST and the preview scenario completed",
            "dev TEST was not both clicked through real app_window input and completed in preview",
        ));

        let real_dev_keyboard = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Keyboard)
        });
        let real_dev_wheel = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Dev
                    && input.real_os
                    && input.kind == InputKind::Wheel)
        });
        self.checks.push(check_result(
            "real-dev-keyboard-and-wheel",
            real_dev_keyboard && real_dev_wheel,
            "kernel keyboard and wheel events reached the focused dev editor through app_window",
            "dev editor did not receive both real keyboard and wheel callbacks",
        ));

        let real_preview = self.events.iter().any(|event| {
            matches!(event, ObserverEvent::InputAccepted(input)
                if input.role == ObserverRole::Preview
                    && input.real_os
                    && input.visible_change)
        });
        self.checks.push(check_result(
            "real-preview-interaction",
            real_preview,
            "preview visible state changed from a real app_window callback",
            "no real preview HostEvent produced a visible frame",
        ));

        let observer_drops = self
            .events
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::FramePresented(frame) => Some(frame.observer_drop_count),
                ObserverEvent::ProofCompleted {
                    result_drop_count, ..
                } => Some(*result_drop_count),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let proof_replaced = self
            .events
            .iter()
            .filter_map(|event| match event {
                ObserverEvent::ProofCompleted { replaced_count, .. } => Some(*replaced_count),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.checks.push(check_result(
            "bounded-observer-and-proof-backpressure",
            observer_drops == 0 && proof_replaced == 0,
            "bounded observer and depth-one proof lanes completed without dropped or replaced evidence",
            format!(
                "observer drops={observer_drops}, proof replacements={proof_replaced}"
            ),
        ));

        let callback_sequences = self.samples.callback_sequences();
        let callback = callback_samples(&self.events, &callback_sequences);
        let visible = preview_visible_frames(&self.events, &self.samples.visible);
        self.checks.push(check_result(
            "minimum-product-samples",
            callback.len() >= 70 && visible.len() >= 70,
            format!(
                "collected {} callback and {} preview visible samples including warmup",
                callback.len(),
                visible.len()
            ),
            format!(
                "insufficient bounded samples: callbacks={}, preview-visible={}",
                callback.len(),
                visible.len()
            ),
        ));
        if gate == Gate::Cells {
            let clicks = preview_visible_frames(&self.events, &self.samples.clicks);
            let scroll = preview_scroll_frames(&self.events, &self.samples.scroll);
            self.checks.push(check_result(
                "minimum-cells-selection-samples",
                clicks.len() >= 24,
                format!("collected {} real cell selection samples", clicks.len()),
                format!(
                    "collected only {} real cell selection samples",
                    clicks.len()
                ),
            ));
            self.checks.push(check_result(
                "minimum-cells-scroll-samples",
                scroll.len() >= 140,
                format!("collected {} real preview scroll samples", scroll.len()),
                format!(
                    "collected only {} real preview scroll samples",
                    scroll.len()
                ),
            ));
        }
        if gate == Gate::CounterDev {
            let ack = switch_ack_samples(&self.events);
            let final_samples = switch_final_samples(&self.events);
            self.checks.push(check_result(
                "minimum-example-switch-samples",
                ack.len() >= 23 && final_samples.len() >= 23,
                format!(
                    "collected {} source acknowledgements and {} final preview switches",
                    ack.len(),
                    final_samples.len()
                ),
                format!(
                    "insufficient switch samples: acknowledgements={}, final={}",
                    ack.len(),
                    final_samples.len()
                ),
            ));
        }

        let exact_proof = exact_proof(&self.events).is_some();
        self.checks.push(check_result(
            "exact-frame-app-owned-proof",
            exact_proof,
            "post-present proof completed for the same FrameEvidenceKey through app-owned WGPU readback",
            "no app-owned proof matched a previously presented product frame identity",
        ));

        if let Some(preview) = metadata.get(&ObserverRole::Preview) {
            self.checks.push(check_result(
                "hardware-adapter",
                !preview.software_adapter && preview.adapter_device_type != "cpu",
                format!(
                    "preview used hardware adapter {} ({})",
                    preview.adapter_name, preview.adapter_device_type
                ),
                format!(
                    "the real COSMIC session exposed software adapter {} ({}); correctness evidence remains valid but the product gate cannot pass",
                    preview.adapter_name, preview.adapter_device_type
                ),
            ));
        }
        self.add_budget_checks(gate);
    }

    fn add_budget_checks(&mut self, gate: Gate) {
        let callback_sequences = self.samples.callback_sequences();
        add_budget_check(
            &mut self.checks,
            "callback-to-host-budget",
            &callback_samples(&self.events, &callback_sequences),
            10,
            60,
            Some(1_000),
            Some(1_000),
            2_000,
        );
        add_frame_budget_check(
            &mut self.checks,
            "warm-visible-budget",
            &preview_visible_frames(&self.events, &self.samples.visible),
            10,
            60,
            16_700,
            33_400,
        );
        if gate == Gate::Cells {
            add_frame_budget_check(
                &mut self.checks,
                "cells-selection-budget",
                &preview_visible_frames(&self.events, &self.samples.clicks),
                4,
                20,
                16_700,
                33_400,
            );
            add_frame_budget_check(
                &mut self.checks,
                "warm-scroll-budget",
                &preview_scroll_frames(&self.events, &self.samples.scroll),
                20,
                120,
                16_700,
                33_400,
            );
        }
        if gate == Gate::CounterDev {
            add_switch_budget_check(
                &mut self.checks,
                "switch-ack-budget",
                &switch_ack_samples(&self.events),
                3,
                20,
                16_700,
                33_400,
            );
            let final_values = switch_final_samples(&self.events)
                .into_iter()
                .map(|sample| sample.elapsed_us)
                .collect::<Vec<_>>();
            add_switch_budget_check(
                &mut self.checks,
                "switch-final-budget",
                &final_values,
                3,
                20,
                250_000,
                500_000,
            );
        }
    }

    fn into_evidence(self, gate: Gate) -> GateEvidence {
        build_gate_evidence(gate, self.checks, &self.events, &self.samples)
    }
}

#[cfg(target_os = "linux")]
fn build_gate_evidence(
    gate: Gate,
    checks: Vec<Check>,
    events: &[ObserverEvent],
    samples: &ProductSamples,
) -> GateEvidence {
    let metadata = role_metadata(events);
    let native = metadata
        .get(&ObserverRole::Preview)
        .zip(metadata.get(&ObserverRole::Dev))
        .and_then(|(preview, dev)| native_evidence(preview, dev.pid));
    let proof = exact_proof(events);
    let mut product_ux_timings = Vec::new();

    let callback_sequences = samples.callback_sequences();
    let callbacks = callback_samples(events, &callback_sequences);
    let visible = preview_visible_frames(events, &samples.visible);
    let proof_frame = proof.as_ref().map(|proof| &proof.key);
    if callbacks.len() >= 70
        && let Some(representative) = representative_callback(&callbacks, &visible, proof_frame)
    {
        let values = callbacks
            .iter()
            .skip(10)
            .map(|sample| sample.input.callback_to_host_ns / 1_000)
            .collect::<Vec<_>>();
        product_ux_timings.push(ProductTimingEvidence {
            metric: "callback-to-host-event",
            representative_frame: representative.frame.key.clone().into(),
            representative_sample_ordinal: representative.ordinal,
            summary: TimingSummary::from_values(&values, 1_000),
        });
    }
    if visible.len() >= 70 {
        let representative_index = proof_frame
            .and_then(|key| visible.iter().position(|frame| &frame.key == key))
            .filter(|index| *index >= 10)
            .unwrap_or(10);
        let values = visible
            .iter()
            .skip(10)
            .map(|sample| sample.input_to_present_us)
            .collect::<Vec<_>>();
        product_ux_timings.push(ProductTimingEvidence {
            metric: "warm-visible-interaction",
            representative_frame: visible[representative_index].key.clone().into(),
            representative_sample_ordinal: (representative_index + 1)
                .try_into()
                .unwrap_or(u32::MAX),
            summary: TimingSummary::from_values(&values, 16_700),
        });
    }
    if gate == Gate::Cells {
        let clicks = preview_visible_frames(events, &samples.clicks);
        if clicks.len() >= 24 {
            let values = clicks
                .iter()
                .skip(4)
                .map(|sample| sample.input_to_present_us)
                .collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "cells-selection",
                representative_frame: clicks[4].key.clone().into(),
                representative_sample_ordinal: 5,
                summary: TimingSummary::from_values(&values, 16_700),
            });
        }
        let scroll = preview_scroll_frames(events, &samples.scroll);
        if scroll.len() >= 140 {
            let values = scroll
                .iter()
                .skip(20)
                .map(|sample| sample.input_to_present_us)
                .collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "warm-scroll",
                representative_frame: scroll[20].key.clone().into(),
                representative_sample_ordinal: 21,
                summary: TimingSummary::from_values(&values, 16_700),
            });
        }
    }
    if gate == Gate::CounterDev {
        let acknowledgements = switch_ack_samples(events);
        let final_samples = switch_final_samples(events);
        if acknowledgements.len() >= 23 && final_samples.len() >= 23 {
            let ack_values = acknowledgements.iter().skip(3).copied().collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "example-switch-acknowledgement",
                representative_frame: final_samples[3].key.clone().into(),
                representative_sample_ordinal: 4,
                summary: TimingSummary::from_values(&ack_values, 16_700),
            });
            let final_values = final_samples
                .iter()
                .skip(3)
                .map(|sample| sample.elapsed_us)
                .collect::<Vec<_>>();
            product_ux_timings.push(ProductTimingEvidence {
                metric: "example-switch-final-preview",
                representative_frame: final_samples[3].key.clone().into(),
                representative_sample_ordinal: 4,
                summary: TimingSummary::from_values(&final_values, 250_000),
            });
        }
    }

    let (async_proof_timing, artifacts) = proof
        .map(|proof| {
            let artifact_id = format!("proof-frame-{}", proof.key.frame_id);
            let completed_after = proof.completed_after_frame_id.max(proof.key.frame_id);
            let lag = completed_after.saturating_sub(proof.key.frame_id);
            let timing = AsyncProofTimingEvidence {
                linked_product_metric: "warm-visible-interaction",
                captured_frame: proof.key.clone().into(),
                completed_after_frame_id: completed_after,
                proof_lag_frames: lag.try_into().unwrap_or(u32::MAX),
                artifact_id: artifact_id.clone(),
                snapshot_prepare_us: proof.snapshot_prepare_us,
                worker_us: proof.elapsed_us,
                summary: TimingSummary::from_values(
                    &[proof.snapshot_prepare_us.saturating_add(proof.elapsed_us)],
                    500_000,
                ),
            };
            let artifact = ArtifactMetadata {
                artifact_id,
                kind: "wgpu-png-readback",
                path: proof.artifact.path.clone(),
                sha256: proof.artifact.sha256.clone(),
                byte_len: proof.artifact.byte_len,
                frame: proof.key.clone().into(),
            };
            (Some(timing), vec![artifact])
        })
        .unwrap_or((None, Vec::new()));

    GateEvidence {
        checks,
        producer: None,
        native,
        product_ux_timings,
        async_proof_timing,
        artifacts,
    }
}

#[cfg(target_os = "linux")]
fn native_evidence(metadata: &RoleMetadata, dev_pid: u32) -> Option<NativeEvidence> {
    let adapter_backend = match metadata.adapter_backend.as_str() {
        "vulkan" | "metal" | "dx12" | "gl" => metadata.adapter_backend.clone(),
        _ => return None,
    };
    let adapter_device_type = match metadata.adapter_device_type.as_str() {
        "integrated-gpu" | "discrete-gpu" | "virtual-gpu" | "cpu" | "other" => {
            metadata.adapter_device_type.clone()
        }
        _ => return None,
    };
    let present_mode = match metadata.present_mode.as_str() {
        "fifo" | "fifo-relaxed" | "immediate" | "mailbox" | "auto-vsync" | "auto-no-vsync" => {
            metadata.present_mode.clone()
        }
        _ => return None,
    };
    Some(NativeEvidence {
        adapter_name: metadata.adapter_name.clone(),
        adapter_backend,
        adapter_device_type,
        software_adapter: metadata.software_adapter,
        present_mode,
        surface_format: metadata.surface_format.clone(),
        window_backend: metadata.window_backend.clone(),
        preview_pid: metadata.pid,
        dev_pid,
        input_delivery: "native-os-app-window-callback",
        scenario_boundary: "public-host-event",
        capture_method: "app-owned-wgpu-readback",
        private_runtime_dispatch_used: false,
    })
}

#[cfg(target_os = "linux")]
fn role_metadata(events: &[ObserverEvent]) -> BTreeMap<ObserverRole, RoleMetadata> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::RoleMetadata(metadata) => Some((metadata.role, metadata.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct ExactProof {
    key: FrameEvidenceKey,
    completed_after_frame_id: u64,
    elapsed_us: u64,
    snapshot_prepare_us: u64,
    artifact: ProofArtifact,
}

#[cfg(target_os = "linux")]
fn exact_proof(events: &[ObserverEvent]) -> Option<ExactProof> {
    events.iter().enumerate().find_map(|(index, event)| {
        let ObserverEvent::ProofCompleted {
            key,
            completed_after_frame_id,
            elapsed_us,
            artifact: Some(artifact),
            error: None,
            ..
        } = event
        else {
            return None;
        };
        let presented_before = events[..index].iter().any(|candidate| {
            matches!(candidate, ObserverEvent::FramePresented(frame) if frame.key == *key)
        });
        let snapshot_prepare_us =
            events[..index]
                .iter()
                .enumerate()
                .find_map(|(request_index, candidate)| {
                    match candidate {
                ObserverEvent::ProofRequested {
                    key: requested,
                    snapshot_prepare_us,
                } if requested == key
                    && events[..request_index].iter().any(|prior| {
                        matches!(prior, ObserverEvent::FramePresented(frame) if frame.key == *key)
                    }) => Some(*snapshot_prepare_us),
                _ => None,
            }
                });
        (presented_before
            && snapshot_prepare_us.is_some()
            && key.is_complete()
            && artifact.byte_len > 0
            && artifact.nonblank_samples > 0
            && artifact.unique_rgba_values > 1)
            .then(|| ExactProof {
                key: key.clone(),
                completed_after_frame_id: *completed_after_frame_id,
                elapsed_us: *elapsed_us,
                snapshot_prepare_us: snapshot_prepare_us.unwrap_or_default(),
                artifact: artifact.clone(),
            })
    })
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct CallbackSample {
    role: ObserverRole,
    input: InputAccepted,
}

#[cfg(target_os = "linux")]
fn callback_samples(events: &[ObserverEvent], sequences: &BTreeSet<u64>) -> Vec<CallbackSample> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::InputAccepted(input)
                if input.real_os
                    && input.role == ObserverRole::Preview
                    && sequences.contains(&input.event_sequence)
                    && matches!(
                        input.kind,
                        InputKind::PointerMove
                            | InputKind::PointerButton
                            | InputKind::Wheel
                            | InputKind::Keyboard
                            | InputKind::Text
                    ) =>
            {
                Some(CallbackSample {
                    role: input.role,
                    input: input.clone(),
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn preview_visible_frames(
    events: &[ObserverEvent],
    sequences: &BTreeSet<u64>,
) -> Vec<FramePresented> {
    real_frames(events, |frame| {
        frame.role == ObserverRole::Preview
            && frame
                .event_sequence
                .is_some_and(|value| sequences.contains(&value))
            && frame
                .input_kind
                .is_some_and(|kind| kind != InputKind::Wheel)
    })
}

#[cfg(target_os = "linux")]
fn preview_scroll_frames(
    events: &[ObserverEvent],
    sequences: &BTreeSet<u64>,
) -> Vec<FramePresented> {
    real_frames(events, |frame| {
        frame.role == ObserverRole::Preview
            && frame
                .event_sequence
                .is_some_and(|value| sequences.contains(&value))
            && frame.input_kind == Some(InputKind::Wheel)
    })
}

#[cfg(target_os = "linux")]
fn real_frames(
    events: &[ObserverEvent],
    predicate: impl Fn(&FramePresented) -> bool,
) -> Vec<FramePresented> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::FramePresented(frame) if predicate(frame) => {
                let real = events.iter().any(|candidate| {
                    matches!(candidate, ObserverEvent::InputAccepted(input)
                        if input.role == frame.role
                            && input.real_os
                            && Some(input.event_sequence) == frame.event_sequence)
                });
                real.then(|| frame.clone())
            }
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn switch_ack_samples(events: &[ObserverEvent]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::SourceSwitchAcknowledged { elapsed_us, .. } => Some(*elapsed_us),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
#[derive(Clone)]
struct SwitchFinalSample {
    revision: u64,
    elapsed_us: u64,
    key: FrameEvidenceKey,
}

#[cfg(target_os = "linux")]
fn switch_final_samples(events: &[ObserverEvent]) -> Vec<SwitchFinalSample> {
    events
        .iter()
        .filter_map(|event| match event {
            ObserverEvent::SourceSwitchFinal {
                revision,
                elapsed_us,
                key,
            } => Some(SwitchFinalSample {
                revision: *revision,
                elapsed_us: *elapsed_us,
                key: key.clone(),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn maximum_switch_revision(events: &[ObserverEvent]) -> u64 {
    switch_final_samples(events)
        .into_iter()
        .map(|sample| sample.revision)
        .max()
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
struct RepresentativeCallback<'a> {
    frame: &'a FramePresented,
    ordinal: u32,
}

#[cfg(target_os = "linux")]
fn representative_callback<'a>(
    callbacks: &[CallbackSample],
    frames: &'a [FramePresented],
    preferred: Option<&FrameEvidenceKey>,
) -> Option<RepresentativeCallback<'a>> {
    let frame = preferred
        .and_then(|key| frames.iter().find(|frame| &frame.key == key))
        .or_else(|| frames.get(10))?;
    let sequence = frame.event_sequence?;
    let ordinal = callbacks
        .iter()
        .position(|sample| sample.role == frame.role && sample.input.event_sequence == sequence)?;
    (ordinal >= 10).then(|| RepresentativeCallback {
        frame,
        ordinal: (ordinal + 1).try_into().unwrap_or(u32::MAX),
    })
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn add_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[CallbackSample],
    warmup: usize,
    minimum: usize,
    p95_limit: Option<u64>,
    p99_limit: Option<u64>,
    max_limit: u64,
) {
    let values = samples
        .iter()
        .skip(warmup)
        .map(|sample| sample.input.callback_to_host_ns / 1_000)
        .collect::<Vec<_>>();
    add_summary_check(
        checks, id, &values, minimum, p95_limit, p99_limit, max_limit,
    );
    if values.len() >= minimum
        && let Some((ordinal, worst)) = samples
            .iter()
            .enumerate()
            .skip(warmup)
            .max_by_key(|(_, sample)| sample.input.callback_to_host_ns)
        && let Some(check) = checks.last_mut()
    {
        check.detail = bounded_detail(format!(
            "{}; worst ordinal={} sequence={} kind={:?} callback={}us target={:?}",
            check.detail,
            ordinal + 1,
            worst.input.event_sequence,
            worst.input.kind,
            worst.input.callback_to_host_ns / 1_000,
            worst.input.target,
        ));
    }
}

#[cfg(target_os = "linux")]
fn add_frame_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[FramePresented],
    warmup: usize,
    minimum: usize,
    p95_limit: u64,
    max_limit: u64,
) {
    let values = samples
        .iter()
        .skip(warmup)
        .map(|sample| sample.input_to_present_us)
        .collect::<Vec<_>>();
    add_summary_check(
        checks,
        id,
        &values,
        minimum,
        Some(p95_limit),
        None,
        max_limit,
    );
    if values.len() >= minimum {
        let component_summary = |values: Vec<u64>| TimingSummary::from_values(&values, p95_limit);
        let frame = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.frame_us)
                .collect(),
        );
        let event_dispatch = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.event_dispatch_us)
                .collect(),
        );
        let executor = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.executor_us)
                .collect(),
        );
        let runtime_document = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.runtime_document_us)
                .collect(),
        );
        let document_update = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.document_update_us)
                .collect(),
        );
        let acquire = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| {
                    sample
                        .frame_us
                        .saturating_sub(sample.render_us + sample.submit_us + sample.present_us)
                })
                .collect(),
        );
        let render = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.render_us)
                .collect(),
        );
        let render_component_p95 = |select: fn(&FramePresented) -> u64| {
            component_summary(samples.iter().skip(warmup).map(select).collect::<Vec<_>>()).p95_us
        };
        let scene_convert = render_component_p95(|sample| sample.document_scene_convert_us);
        let scene_key = render_component_p95(|sample| sample.scene_key_us);
        let rect_vertices = render_component_p95(|sample| sample.rect_vertices_us);
        let asset_prepare = render_component_p95(|sample| sample.asset_prepare_us);
        let quad_batch_key = render_component_p95(|sample| sample.quad_batch_key_us);
        let quad_upload = render_component_p95(|sample| sample.quad_upload_us);
        let draw_pass = render_component_p95(|sample| sample.draw_pass_us);
        let retained_metrics = render_component_p95(|sample| sample.retained_metrics_us);
        let text_render = render_component_p95(|sample| sample.text_render_us);
        let submit = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.submit_us)
                .collect(),
        );
        let present = component_summary(
            samples
                .iter()
                .skip(warmup)
                .map(|sample| sample.present_us)
                .collect(),
        );
        if let Some(check) = checks.last_mut() {
            check.detail = bounded_detail(format!(
                "{}; component p95/max: dispatch={}/{}us executor={}/{}us runtime_document={}/{}us retained={}/{}us frame={}/{}us acquire={}us render={}us submit={}us present={}us; render p95: convert={}us scene_key={}us rects={}us assets={}us batch_key={}us upload={}us draw={}us metrics={}us text={}us",
                check.detail,
                event_dispatch.p95_us,
                event_dispatch.max_us,
                executor.p95_us,
                executor.max_us,
                runtime_document.p95_us,
                runtime_document.max_us,
                document_update.p95_us,
                document_update.max_us,
                frame.p95_us,
                frame.max_us,
                acquire.p95_us,
                render.p95_us,
                submit.p95_us,
                present.p95_us,
                scene_convert,
                scene_key,
                rect_vertices,
                asset_prepare,
                quad_batch_key,
                quad_upload,
                draw_pass,
                retained_metrics,
                text_render,
            ));
        }
    }
}

#[cfg(target_os = "linux")]
fn add_switch_budget_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    samples: &[u64],
    warmup: usize,
    minimum: usize,
    p95_limit: u64,
    max_limit: u64,
) {
    let values = samples.iter().skip(warmup).copied().collect::<Vec<_>>();
    add_summary_check(
        checks,
        id,
        &values,
        minimum,
        Some(p95_limit),
        None,
        max_limit,
    );
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn add_summary_check(
    checks: &mut Vec<Check>,
    id: &'static str,
    values: &[u64],
    minimum: usize,
    p95_limit: Option<u64>,
    p99_limit: Option<u64>,
    max_limit: u64,
) {
    if values.len() < minimum {
        checks.push(Check::fail(
            id,
            format!(
                "{} samples after warmup; minimum is {minimum}",
                values.len()
            ),
        ));
        return;
    }
    let summary = TimingSummary::from_values(values, p95_limit.unwrap_or(max_limit));
    let pass = p95_limit.is_none_or(|limit| summary.p95_us <= limit)
        && p99_limit.is_none_or(|limit| summary.p99_us <= limit)
        && summary.max_us <= max_limit;
    checks.push(check_result(
        id,
        pass,
        format!(
            "{} samples: p95={}us p99={}us max={}us",
            summary.sample_count, summary.p95_us, summary.p99_us, summary.max_us
        ),
        format!(
            "{} samples exceed budget: p95={}us p99={}us max={}us",
            summary.sample_count, summary.p95_us, summary.p99_us, summary.max_us
        ),
    ));
}

#[cfg(target_os = "linux")]
struct ObserverServer {
    socket_path: PathBuf,
    receiver: mpsc::Receiver<Result<ObserverEvent, String>>,
    closing: Arc<AtomicBool>,
    acceptor: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl ObserverServer {
    fn bind(path: &Path) -> Result<Self, String> {
        let _ = fs::remove_file(path);
        let listener = UnixListener::bind(path)
            .map_err(|error| format!("bind observer {}: {error}", path.display()))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let (sender, receiver) = mpsc::sync_channel(OBSERVER_QUEUE_DEPTH);
        let closing = Arc::new(AtomicBool::new(false));
        let accept_closing = Arc::clone(&closing);
        let acceptor = thread::Builder::new()
            .name("boon-verifier-observer-server".to_owned())
            .spawn(move || {
                while !accept_closing.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let sender = sender.clone();
                            let _ = thread::Builder::new()
                                .name("boon-verifier-observer-reader".to_owned())
                                .spawn(move || observer_reader(stream, sender));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => {
                            let _ = sender.send(Err(format!("observer accept failed: {error}")));
                            return;
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            socket_path: path.to_owned(),
            receiver,
            closing,
            acceptor: Some(acceptor),
        })
    }

    fn recv_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Result<ObserverEvent, String>, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

#[cfg(target_os = "linux")]
impl Drop for ObserverServer {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::Relaxed);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

#[cfg(target_os = "linux")]
fn observer_reader(
    mut stream: std::os::unix::net::UnixStream,
    sender: mpsc::SyncSender<Result<ObserverEvent, String>>,
) {
    loop {
        match read_event(&mut stream) {
            Ok(Some(event)) => {
                if sender.send(Ok(event)).is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(error) => {
                let _ = sender.send(Err(error.to_string()));
                return;
            }
        }
    }
}

#[cfg(target_os = "linux")]
struct ScratchDir {
    path: PathBuf,
}

#[cfg(target_os = "linux")]
impl ScratchDir {
    fn create(run_id: &str, gate: &str) -> Result<Self, String> {
        use std::os::unix::fs::PermissionsExt;

        let parent = std::env::temp_dir().join("boon-native-v2");
        fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let stem = safe_component(&format!("{run_id}-{gate}-{}-{nonce}", std::process::id()));
        for suffix in 0..16_u8 {
            let path = parent.join(format!("{stem}-{suffix}"));
            match fs::create_dir(&path) {
                Ok(()) => {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                        .map_err(|error| error.to_string())?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("create {}: {error}", path.display())),
            }
        }
        Err("cannot allocate a unique native verifier scratch directory".to_owned())
    }
}

#[cfg(target_os = "linux")]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        if std::env::var_os("BOON_VERIFY_KEEP_SCRATCH").is_none() {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            eprintln!("kept verifier scratch at {}", self.path.display());
        }
    }
}

#[cfg(target_os = "linux")]
struct NativeSession {
    desktop_pid: u32,
    launch_id: String,
    isolated_seat_name: String,
    observed_roles: Vec<u32>,
    input: Option<NativeInput>,
    workspace: Option<WorkspaceGuard>,
    pointer_space: Option<(i32, i32)>,
    closed: bool,
}

#[cfg(target_os = "linux")]
impl NativeSession {
    fn start(
        workspace: &Path,
        runtime_dir: &Path,
        executable: &Path,
        example: &str,
        observer_socket: &Path,
        artifact_dir: &Path,
    ) -> Result<Self, String> {
        let ipc = runtime_dir.join("desktop.sock");
        let launch_log = runtime_dir.join("desktop-launch.log");
        let environment = [
            (
                OBSERVER_SOCKET_ENV,
                observer_socket.to_string_lossy().into_owned(),
            ),
            (PROOF_MODE_ENV, "readback".to_owned()),
            (
                PROOF_ARTIFACT_DIR_ENV,
                artifact_dir.to_string_lossy().into_owned(),
            ),
            (PROOF_SAMPLE_ORDINAL_ENV, "64".to_owned()),
            (crate::protocol::VERIFY_BOUNDED_WINDOWS_ENV, "1".to_owned()),
        ];
        let mut launcher = Command::new("cosmic-background-launch");
        launcher.current_dir(workspace).args([
            "--workspace",
            NATIVE_WORKSPACE,
            "--frame-pacing",
            "demand",
            "--isolated-input",
            "--",
            "env",
        ]);
        for (name, value) in environment {
            launcher.arg(format!("{name}={value}"));
        }
        launcher
            .arg(executable)
            .args(["--role", "desktop", "--example", example, "--ipc-path"])
            .arg(ipc);
        let result = run_logged(&mut launcher, &launch_log, Duration::from_secs(10))
            .map_err(|error| format!("launch isolated COSMIC windows: {error}"))?;
        if !result.success() {
            return Err(process_failure(
                "cosmic-background-launch",
                &result,
                &launch_log,
            ));
        }
        let mut launch_fields = result.output.split_whitespace();
        let desktop_pid = launch_fields
            .next()
            .ok_or("cosmic-background-launch omitted the desktop PID")?
            .parse::<u32>()
            .map_err(|error| {
                format!("invalid desktop PID from cosmic-background-launch: {error}")
            })?;
        let launch_id = launch_fields
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("cosmic-background-launch omitted the launch ID")?
            .to_owned();
        let isolated_seat_name = launch_fields
            .next()
            .filter(|value| !value.is_empty())
            .ok_or("cosmic-background-launch omitted the isolated seat name")?
            .to_owned();
        if launch_fields.next().is_some() {
            terminate_process(desktop_pid, "TERM");
            let _ = release_background_launch(&launch_id);
            return Err("cosmic-background-launch returned unexpected fields".to_owned());
        }
        let input = match NativeInput::start(executable, &isolated_seat_name) {
            Ok(input) => input,
            Err(error) => {
                terminate_process(desktop_pid, "TERM");
                let _ = release_background_launch(&launch_id);
                return Err(error);
            }
        };
        if let Err(error) = wait_for_isolated_input(&launch_id, &isolated_seat_name) {
            let mut input = input;
            let _ = input.shutdown();
            terminate_process(desktop_pid, "TERM");
            let _ = release_background_launch(&launch_id);
            return Err(error);
        }
        Ok(Self {
            desktop_pid,
            launch_id,
            isolated_seat_name,
            observed_roles: Vec::new(),
            input: Some(input),
            workspace: None,
            pointer_space: None,
            closed: false,
        })
    }

    fn desktop_id(&self) -> u32 {
        self.desktop_pid
    }

    fn prepare_background_workspace(&mut self, executable: &Path) -> Result<(), String> {
        self.reconcile_background_layout()?;
        let workspace = WorkspaceGuard::start(executable, NATIVE_WORKSPACE)?;
        let (width, height) = workspace.output_size();
        let input = self
            .input
            .as_mut()
            .ok_or("kernel virtual input process is unavailable")?;
        input.set_pointer_space(width, height)?;
        input.prepare_pointer()?;
        self.pointer_space = Some((width, height));
        self.workspace = Some(workspace);
        Ok(())
    }

    fn reconcile_background_layout(&self) -> Result<(), String> {
        let output = Command::new("cosmic-background-launch")
            .args(["--reconcile", &self.launch_id])
            .output()
            .map_err(|error| format!("reconcile COSMIC background launch: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "COSMIC background launch reconciliation failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let reconciled = String::from_utf8(output.stdout)
            .map_err(|error| format!("invalid COSMIC reconciliation output: {error}"))?
            .trim()
            .parse::<usize>()
            .map_err(|error| format!("invalid COSMIC reconciliation count: {error}"))?;
        if reconciled < self.observed_roles.len() {
            return Err(format!(
                "COSMIC reconciled only {reconciled} of {} native role windows",
                self.observed_roles.len()
            ));
        }
        let isolation = query_isolation_status(&self.launch_id)?;
        isolation.require_safe(&self.isolated_seat_name)?;
        isolation.require_layout(self.observed_roles.len())?;
        Ok(())
    }

    fn wait_for_roles(&mut self, timeout: Duration) -> Result<RolePids, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if !process_exists(self.desktop_id()) {
                return Err("desktop exited before preview and dev connected".to_owned());
            }
            let descendants = process_descendants(self.desktop_id());
            let preview = descendants
                .iter()
                .copied()
                .find(|pid| process_role(*pid).as_deref() == Some("preview"));
            let dev = descendants
                .iter()
                .copied()
                .find(|pid| process_role(*pid).as_deref() == Some("dev"));
            if let (Some(preview), Some(dev)) = (preview, dev) {
                self.observed_roles = vec![preview, dev];
                return Ok(RolePids { preview, dev });
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "did not observe distinct preview/dev children within {}ms; descendants={descendants:?}",
                    timeout.as_millis()
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn run_driver(&mut self, arguments: &[&str]) -> Result<DriverAck, String> {
        let input = self
            .input
            .as_mut()
            .ok_or("kernel virtual input process is unavailable")?;
        match arguments {
            ["move", x, y] => {
                let point = (
                    x.parse::<i32>().map_err(|error| error.to_string())?,
                    y.parse::<i32>().map_err(|error| error.to_string())?,
                );
                let actual = input.move_pointer(point)?;
                Ok(DriverAck {
                    output: format!("x={} y={}", actual.0, actual.1),
                })
            }
            ["click", button] => {
                input.click(pointer_button_code(button)?)?;
                Ok(DriverAck::default())
            }
            ["button", state, button] => {
                input.button(pointer_button_code(button)?, *state == "down")?;
                Ok(DriverAck::default())
            }
            ["button", state] => {
                input.button(0x110, *state == "down")?;
                Ok(DriverAck::default())
            }
            ["axis", axis, amount] => {
                input.wheel(
                    *axis == "horizontal",
                    amount.parse().map_err(|error| {
                        format!("invalid virtual wheel amount `{amount}`: {error}")
                    })?,
                )?;
                Ok(DriverAck::default())
            }
            ["chord", modifier, key] => {
                input.chord(&[key_code(modifier)?], key_code(key)?)?;
                Ok(DriverAck::default())
            }
            ["key", state, key] => {
                input.key(key_code(key)?, *state == "down")?;
                Ok(DriverAck::default())
            }
            _ => Err(format!(
                "unsupported kernel virtual input command: {}",
                arguments.join(" ")
            )),
        }
    }

    fn move_pointer(&mut self, point: (i32, i32)) -> Result<(i32, i32), String> {
        let x = point.0.to_string();
        let y = point.1.to_string();
        let result = self.run_driver(&["move", &x, &y])?;
        let coordinate = |prefix: &str| -> Result<i32, String> {
            let value = result
                .output
                .split_whitespace()
                .find_map(|part| part.strip_prefix(prefix))
                .ok_or_else(|| format!("driver move acknowledgement omitted {prefix}"))?
                .parse::<f64>()
                .map_err(|error| format!("invalid driver move {prefix} coordinate: {error}"))?;
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(format!("driver move {prefix} coordinate is out of range"));
            }
            Ok(value.round() as i32)
        };
        Ok((coordinate("x=")?, coordinate("y=")?))
    }

    fn pointer_space(&self) -> Result<(i32, i32), String> {
        self.pointer_space.ok_or(
            "native pointer space is unavailable before isolated layout preparation".to_owned(),
        )
    }

    fn shutdown(&mut self) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut errors = Vec::new();
        if let Some(mut input) = self.input.take()
            && let Err(error) = input.shutdown()
        {
            errors.push(error);
        }
        if let Some(mut workspace) = self.workspace.take()
            && let Err(error) = workspace.shutdown()
        {
            errors.push(error);
        }
        let desktop_id = self.desktop_id();
        let mut pids = process_descendants(desktop_id);
        pids.extend(self.observed_roles.iter().copied());
        pids.push(desktop_id);
        pids.sort_unstable();
        pids.dedup();
        for pid in pids.iter().rev().copied().filter(|pid| *pid != 0) {
            terminate_process(pid, "TERM");
        }
        let deadline = Instant::now() + CLEANUP_TIMEOUT;
        while pids.iter().copied().any(process_exists) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        for pid in pids
            .iter()
            .rev()
            .copied()
            .filter(|pid| process_exists(*pid))
        {
            terminate_process(pid, "KILL");
        }
        if let Err(error) = release_background_launch(&self.launch_id) {
            errors.push(error);
        }
        self.desktop_pid = 0;
        let leaked = pids
            .into_iter()
            .filter(|pid| *pid != 0 && process_exists(*pid))
            .collect::<Vec<_>>();
        if !leaked.is_empty() {
            errors.push(format!(
                "native verifier process cleanup left live PIDs {leaked:?}"
            ));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(target_os = "linux")]
struct IsolationStatus {
    seat_name: String,
    device_count: usize,
    workspace_active: bool,
    mapped_surface_count: usize,
    tiling_enabled: bool,
    floating_window_count: usize,
    tiled_window_count: usize,
    maximized_window_count: usize,
}

#[cfg(target_os = "linux")]
impl IsolationStatus {
    fn require_safe(&self, expected_seat: &str) -> Result<(), String> {
        if self.seat_name != expected_seat {
            return Err(format!(
                "isolated input status named seat `{}`, expected `{expected_seat}`",
                self.seat_name
            ));
        }
        if self.device_count != 2 {
            return Err(format!(
                "isolated seat `{expected_seat}` owns {} devices, expected pointer and keyboard",
                self.device_count
            ));
        }
        if self.workspace_active {
            return Err(format!(
                "isolated seat `{expected_seat}` targets the active workspace; refusing input"
            ));
        }
        Ok(())
    }

    fn require_layout(&self, expected_windows: usize) -> Result<(), String> {
        if !self.tiling_enabled
            || self.floating_window_count != 0
            || self.tiled_window_count < expected_windows
            || self.maximized_window_count != 0
            || self.mapped_surface_count < expected_windows
        {
            return Err(format!(
                "isolated workspace layout is not independently tiled: mapped={}, tiled={}, \
                 floating={}, maximized={}, tiling_enabled={}, expected_windows={expected_windows}",
                self.mapped_surface_count,
                self.tiled_window_count,
                self.floating_window_count,
                self.maximized_window_count,
                self.tiling_enabled,
            ));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn wait_for_isolated_input(
    launch_id: &str,
    expected_seat: &str,
) -> Result<IsolationStatus, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let error = match query_isolation_status(launch_id) {
            Ok(status) => match status.require_safe(expected_seat) {
                Ok(()) => return Ok(status),
                Err(error) => error,
            },
            Err(error) => error,
        };
        if Instant::now() >= deadline {
            return Err(error);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(target_os = "linux")]
fn query_isolation_status(launch_id: &str) -> Result<IsolationStatus, String> {
    let output = Command::new("cosmic-background-launch")
        .args(["--isolation-status", launch_id])
        .output()
        .map_err(|error| format!("query COSMIC input isolation: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "COSMIC input isolation query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| format!("invalid COSMIC isolation status: {error}"))?;
    let value = |name: &str| {
        output
            .split_whitespace()
            .find_map(|field| field.strip_prefix(name))
            .ok_or_else(|| format!("COSMIC isolation status omitted {name}"))
    };
    Ok(IsolationStatus {
        seat_name: value("seat=")?.to_owned(),
        device_count: value("devices=")?
            .parse()
            .map_err(|error| format!("invalid isolated device count: {error}"))?,
        workspace_active: value("workspace_active=")?
            .parse()
            .map_err(|error| format!("invalid isolated workspace state: {error}"))?,
        mapped_surface_count: value("mapped_surfaces=")?
            .parse()
            .map_err(|error| format!("invalid isolated mapped-surface count: {error}"))?,
        tiling_enabled: value("tiling_enabled=")?
            .parse()
            .map_err(|error| format!("invalid isolated tiling state: {error}"))?,
        floating_window_count: value("floating_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated floating-window count: {error}"))?,
        tiled_window_count: value("tiled_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated tiled-window count: {error}"))?,
        maximized_window_count: value("maximized_windows=")?
            .parse()
            .map_err(|error| format!("invalid isolated maximized-window count: {error}"))?,
    })
}

#[cfg(target_os = "linux")]
fn release_background_launch(launch_id: &str) -> Result<(), String> {
    let output = Command::new("cosmic-background-launch")
        .args(["--release", launch_id])
        .output()
        .map_err(|error| format!("release COSMIC background launch: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "release COSMIC background launch failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "linux")]
#[derive(Default)]
struct DriverAck {
    output: String,
}

#[cfg(target_os = "linux")]
fn pointer_button_code(name: &str) -> Result<u16, String> {
    match name {
        "left" => Ok(0x110),
        "right" => Ok(0x111),
        "middle" => Ok(0x112),
        value => value
            .parse()
            .map_err(|error| format!("invalid pointer button `{value}`: {error}")),
    }
}

#[cfg(target_os = "linux")]
fn key_code(name: &str) -> Result<u16, String> {
    match name {
        "tab" => Ok(15),
        "enter" => Ok(28),
        "escape" => Ok(1),
        "left" => Ok(105),
        "right" => Ok(106),
        "i" => Ok(23),
        "u" => Ok(22),
        "y" => Ok(21),
        value => value
            .parse()
            .map_err(|error| format!("invalid keyboard key `{value}`: {error}")),
    }
}

#[cfg(target_os = "linux")]
impl Drop for NativeSession {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct RolePids {
    preview: u32,
    dev: u32,
}

#[cfg(target_os = "linux")]
struct LoggedProcess {
    status: ExitStatus,
    timed_out: bool,
    output: String,
}

#[cfg(target_os = "linux")]
impl LoggedProcess {
    fn success(&self) -> bool {
        self.status.success() && !self.timed_out
    }
}

#[cfg(target_os = "linux")]
fn cleanup_check(result: Result<(), String>) -> Check {
    match result {
        Ok(()) => Check::pass(
            "native-os-input-cleanup",
            "isolated virtual devices and desktop/preview/dev process tree stopped without leaks or workspace activation",
        ),
        Err(error) => Check::fail("native-os-input-cleanup", error),
    }
}

#[cfg(target_os = "linux")]
fn run_logged(
    command: &mut Command,
    log_path: &Path,
    timeout: Duration,
) -> std::io::Result<LoggedProcess> {
    let log = File::create(log_path)?;
    command
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    let started = Instant::now();
    let mut child = command.spawn()?;
    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if started.elapsed() >= timeout {
            child.kill()?;
            break (child.wait()?, true);
        }
        thread::sleep(Duration::from_millis(10));
    };
    Ok(LoggedProcess {
        status,
        timed_out,
        output: fs::read_to_string(log_path).unwrap_or_default(),
    })
}

#[cfg(target_os = "linux")]
fn process_failure(label: &str, process: &LoggedProcess, log: &Path) -> String {
    bounded_detail(format!(
        "{label} failed{} with {}; {}",
        if process.timed_out {
            " after timeout"
        } else {
            ""
        },
        process.status,
        tail(log, 2_000)
    ))
}

#[cfg(target_os = "linux")]
fn process_descendants(root: u32) -> Vec<u32> {
    if root == 0 {
        return Vec::new();
    }
    let mut found = BTreeSet::new();
    let mut pending = VecDeque::from([root]);
    while let Some(parent) = pending.pop_front() {
        let children_path = format!("/proc/{parent}/task/{parent}/children");
        let Ok(children) = fs::read_to_string(children_path) else {
            continue;
        };
        for child in children
            .split_whitespace()
            .filter_map(|value| value.parse::<u32>().ok())
        {
            if found.insert(child) {
                pending.push_back(child);
            }
        }
    }
    found.into_iter().collect()
}

#[cfg(target_os = "linux")]
fn process_role(pid: u32) -> Option<String> {
    let bytes = fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let arguments = bytes
        .split(|byte| *byte == 0)
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf8_lossy(value).into_owned())
        .collect::<Vec<_>>();
    arguments
        .windows(2)
        .find(|pair| pair[0] == "--role")
        .map(|pair| pair[1].clone())
}

#[cfg(target_os = "linux")]
fn process_exists(pid: u32) -> bool {
    pid != 0 && Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(target_os = "linux")]
fn terminate_process(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "linux")]
fn tail(path: &Path, maximum: usize) -> String {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) => return format!("cannot read {}: {error}", path.display()),
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.read_to_end(&mut bytes) {
        return format!("cannot read {}: {error}", path.display());
    }
    let start = bytes.len().saturating_sub(maximum);
    String::from_utf8_lossy(&bytes[start..]).trim().to_owned()
}

fn write_envelope(path: &Path, envelope: &ProducerEnvelope) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(envelope).map_err(|error| error.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err(format!(
            "producer evidence is unbounded at {} bytes",
            bytes.len()
        ));
    }
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn required<'a>(args: &'a [String], flag: &str) -> Result<&'a str, String> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
        .ok_or_else(|| format!("{flag} requires a value"))
}

#[cfg(any(target_os = "linux", test))]
fn safe_component(value: &str) -> String {
    let value = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .take(48)
        .collect::<String>();
    if value.is_empty() {
        "native-v2".to_owned()
    } else {
        value
    }
}

fn bounded_detail(value: impl Into<String>) -> String {
    let value = value.into();
    if value.len() <= MAX_DETAIL_BYTES {
        return value;
    }
    let mut end = MAX_DETAIL_BYTES.saturating_sub(3);
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &value[..end])
}

fn check_result(
    id: &'static str,
    pass: bool,
    pass_detail: impl Into<String>,
    fail_detail: impl Into<String>,
) -> Check {
    if pass {
        Check::pass(id, pass_detail)
    } else {
        Check::fail(id, fail_detail)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gate {
    CounterDev,
    TodomvcPhysical,
    Cells,
    Novywave,
    Negative,
}

impl Gate {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "counter-dev" => Ok(Self::CounterDev),
            "todomvc-physical" => Ok(Self::TodomvcPhysical),
            "cells" => Ok(Self::Cells),
            "novywave" => Ok(Self::Novywave),
            "negative" => Ok(Self::Negative),
            _ => Err(format!("unsupported v2 producer gate `{value}`")),
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Self::CounterDev => "counter-dev",
            Self::TodomvcPhysical => "todomvc-physical",
            Self::Cells => "cells",
            Self::Novywave => "novywave",
            Self::Negative => "negative",
        }
    }

    #[cfg(any(target_os = "linux", test))]
    fn example_id(self) -> &'static str {
        match self {
            Self::CounterDev | Self::Negative => "counter",
            Self::TodomvcPhysical => "todo_mvc_physical",
            Self::Cells => "cells",
            Self::Novywave => "novywave",
        }
    }

    fn is_timed(self) -> bool {
        self != Self::Negative
    }
}

#[derive(Serialize)]
struct ProducerEnvelope {
    format: u16,
    protocol: &'static str,
    gate: &'static str,
    run_id: String,
    source_digest: String,
    evidence: GateEvidence,
}

#[derive(Serialize)]
struct GateEvidence {
    checks: Vec<Check>,
    producer: Option<()>,
    native: Option<NativeEvidence>,
    product_ux_timings: Vec<ProductTimingEvidence>,
    async_proof_timing: Option<AsyncProofTimingEvidence>,
    artifacts: Vec<ArtifactMetadata>,
}

impl GateEvidence {
    #[cfg(not(target_os = "linux"))]
    fn failed(check: Check) -> Self {
        Self {
            checks: vec![check],
            producer: None,
            native: None,
            product_ux_timings: Vec::new(),
            async_proof_timing: None,
            artifacts: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct NativeEvidence {
    adapter_name: String,
    adapter_backend: String,
    adapter_device_type: String,
    software_adapter: bool,
    present_mode: String,
    surface_format: String,
    window_backend: String,
    preview_pid: u32,
    dev_pid: u32,
    input_delivery: &'static str,
    scenario_boundary: &'static str,
    capture_method: &'static str,
    private_runtime_dispatch_used: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReportFrameEvidenceKey {
    frame_id: u64,
    input_id: u64,
    content_id: u64,
    layout_id: u64,
    render_id: u64,
    surface_epoch: u64,
    present_id: u64,
    proof_id: u64,
}

#[cfg(target_os = "linux")]
impl From<FrameEvidenceKey> for ReportFrameEvidenceKey {
    fn from(value: FrameEvidenceKey) -> Self {
        Self {
            frame_id: value.frame_id,
            input_id: value.input_id,
            content_id: value.content_id,
            layout_id: value.layout_id,
            render_id: value.render_id,
            surface_epoch: value.surface_epoch,
            present_id: value.present_id,
            proof_id: value.proof_id,
        }
    }
}

#[derive(Serialize)]
struct ProductTimingEvidence {
    metric: &'static str,
    representative_frame: ReportFrameEvidenceKey,
    representative_sample_ordinal: u32,
    summary: TimingSummary,
}

#[derive(Serialize)]
struct AsyncProofTimingEvidence {
    linked_product_metric: &'static str,
    captured_frame: ReportFrameEvidenceKey,
    completed_after_frame_id: u64,
    proof_lag_frames: u32,
    artifact_id: String,
    snapshot_prepare_us: u64,
    worker_us: u64,
    summary: TimingSummary,
}

#[derive(Serialize)]
struct ArtifactMetadata {
    artifact_id: String,
    kind: &'static str,
    path: String,
    sha256: String,
    byte_len: u64,
    frame: ReportFrameEvidenceKey,
}

#[derive(Clone, Debug, Serialize)]
struct TimingSummary {
    sample_count: u32,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
    outlier_count: u32,
}

impl TimingSummary {
    fn from_values(values: &[u64], outlier_threshold_us: u64) -> Self {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        Self {
            sample_count: sorted.len().try_into().unwrap_or(u32::MAX),
            p50_us: nearest_rank(&sorted, 50),
            p95_us: nearest_rank(&sorted, 95),
            p99_us: nearest_rank(&sorted, 99),
            max_us: sorted.last().copied().unwrap_or(0),
            outlier_count: sorted
                .iter()
                .filter(|value| **value > outlier_threshold_us)
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
        }
    }
}

fn nearest_rank(sorted: &[u64], percentile: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = percentile.saturating_mul(sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Serialize)]
struct Check {
    id: &'static str,
    outcome: &'static str,
    detail: String,
}

impl Check {
    fn pass(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            outcome: "pass",
            detail: bounded_detail(detail),
        }
    }

    fn fail(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            outcome: "fail",
            detail: bounded_detail(detail),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_mapping_is_generic_and_complete() {
        for (slug, example, timed) in [
            ("counter-dev", "counter", true),
            ("todomvc-physical", "todo_mvc_physical", true),
            ("cells", "cells", true),
            ("novywave", "novywave", true),
            ("negative", "counter", false),
        ] {
            let gate = Gate::parse(slug).expect("known gate");
            assert_eq!(gate.slug(), slug);
            assert_eq!(gate.example_id(), example);
            assert_eq!(gate.is_timed(), timed);
        }
        assert!(Gate::parse("architecture").is_err());
    }

    #[test]
    fn details_remain_valid_utf8_and_schema_bounded() {
        let detail = bounded_detail("cells euro".repeat(400));
        assert!(detail.len() <= MAX_DETAIL_BYTES);
        assert!(detail.ends_with("..."));
    }

    #[test]
    fn summaries_retain_outliers_and_use_nearest_rank() {
        let summary = TimingSummary::from_values(&[1, 2, 3, 4, 100], 10);
        assert_eq!(summary.p50_us, 3);
        assert_eq!(summary.p95_us, 100);
        assert_eq!(summary.outlier_count, 1);

        let mut p99_samples = vec![50; 109];
        p99_samples.push(1_500);
        let summary = TimingSummary::from_values(&p99_samples, 1_000);
        assert_eq!(summary.p99_us, 50);
        assert_eq!(summary.max_us, 1_500);
    }

    #[test]
    fn scratch_names_cannot_escape_the_runtime_directory() {
        assert_eq!(safe_component("../../run/id"), "______run_id");
        assert_eq!(safe_component("cells-01"), "cells-01");
    }

    #[test]
    fn window_scan_covers_the_entire_reported_output() {
        let candidates = window_scan_candidates((5_120, 1_440));
        assert!(candidates.iter().any(|(x, _)| *x > 3_500));
        assert!(candidates.iter().any(|(_, y)| *y > 1_000));
        assert!(
            candidates
                .iter()
                .all(|(x, y)| (0..5_120).contains(x) && (0..1_440).contains(y))
        );
    }
}
